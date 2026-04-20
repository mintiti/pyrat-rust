use std::collections::{HashMap, HashSet};
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, info, info_span, warn, Instrument};

use crate::session::messages::HostCommand;
use crate::session::{run_session, SessionConfig, SessionId, SessionMsg};
use pyrat_protocol::{HashedTurnState, OwnedTurnState};

use pyrat_wire::Player;

use super::config::{MatchSetup, SessionHandle};
use super::events::{emit, MatchEvent};
use super::slots::PlayerSlots;

// ── Public types ─────────────────────────────────────

/// Successful result of the setup phase.
#[derive(Debug)]
pub struct SetupResult {
    pub sessions: Vec<SessionHandle>,
}

/// What can go wrong during setup.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("startup timeout — unclaimed slots: {unclaimed:?}")]
    StartupTimeout { unclaimed: Vec<Player> },
    #[error("all sessions disconnected")]
    AllDisconnected,
    #[error("bot {agent_id:?} ({name:?}) disconnected during setup")]
    BotDisconnected { name: String, agent_id: String },
    #[error("preprocessing timeout — pending bots: {pending:?}")]
    PreprocessingTimeout { pending: Vec<String> },
}

// ── Pending session (connected but not yet fully set up) ─

struct PendingSession {
    cmd_tx: mpsc::Sender<HostCommand>,
}

// ── Setup phase ──────────────────────────────────────

/// Run the setup phase: accept bot connections, assign player slots,
/// configure bots, and wait for preprocessing to finish.
///
/// The caller must separately spawn `accept_connections` (or use duplex
/// pairs in tests) so that `SessionMsg`s arrive on `game_rx`.
///
/// Bots may send Ready immediately after Identify (the session state machine
/// permits it). This function buffers early Ready and PreprocessingDone
/// messages so they aren't lost between phases.
pub async fn run_setup(
    setup: &MatchSetup,
    game_rx: &mut mpsc::Receiver<SessionMsg>,
    event_tx: Option<&mpsc::UnboundedSender<MatchEvent>>,
) -> Result<SetupResult, SetupError> {
    let setup_start = Instant::now();
    let startup_deadline = Instant::now() + setup.timing.startup_timeout;
    let mut slots = PlayerSlots::new(&setup.players);

    // Pending sessions that connected but haven't identified yet.
    let mut pending: HashMap<SessionId, PendingSession> = HashMap::new();
    // Sessions that identified and claimed slots.
    let mut handles: HashMap<SessionId, SessionHandle> = HashMap::new();
    // Buffer for Ready messages that arrive before phase B.
    let mut ready_set: HashSet<SessionId> = HashSet::new();
    // Buffer for PreprocessingDone messages that arrive before phase C.
    let mut done_set: HashSet<SessionId> = HashSet::new();

    // ── Phase A: Wait for Identify + reserve slots ───
    //
    // Also buffers Ready/PreprocessingDone that arrive early.
    loop {
        tokio::select! {
            msg = game_rx.recv() => {
                let Some(msg) = msg else {
                    return Err(SetupError::AllDisconnected);
                };
                match msg {
                    SessionMsg::Connected { session_id, cmd_tx } => {
                        debug!(session = session_id.0, "bot connected (setup phase A)");
                        pending.insert(session_id, PendingSession { cmd_tx });
                    }
                    SessionMsg::Identified { session_id, name, author, options: _, agent_id } => {
                        if let Some(ps) = pending.remove(&session_id) {
                            let claimed = slots.reserve(session_id, &agent_id);
                            if claimed.is_empty() {
                                warn!(
                                    session = session_id.0,
                                    agent_id = %agent_id,
                                    "unknown agent_id — dropping session"
                                );
                                drop(ps.cmd_tx);
                            } else {
                                info!(
                                    session = session_id.0,
                                    agent_id = %agent_id,
                                    players = ?claimed,
                                    "assigned to player slot(s)"
                                );
                                for &p in &claimed {
                                    emit(event_tx, MatchEvent::BotIdentified {
                                        player: p,
                                        name: name.clone(),
                                        author: author.clone(),
                                    });
                                }
                                handles.insert(session_id, SessionHandle {
                                    session_id,
                                    cmd_tx: ps.cmd_tx,
                                    name,
                                    author,
                                    agent_id,
                                    controlled_players: claimed,
                                });
                                if slots.all_claimed() {
                                    break;
                                }
                            }
                        }
                    }
                    SessionMsg::Ready { session_id } => {
                        ready_set.insert(session_id);
                    }
                    SessionMsg::PreprocessingDone { session_id } => {
                        done_set.insert(session_id);
                    }
                    SessionMsg::Disconnected { session_id, reason } => {
                        pending.remove(&session_id);
                        if let Some(h) = handles.remove(&session_id) {
                            for &p in &h.controlled_players {
                                emit(event_tx, MatchEvent::BotDisconnected { player: p, reason });
                            }
                            slots.unreserve(session_id);
                        }
                    }
                    // Info, Action, etc. — ignored during setup.
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(startup_deadline) => {
                return Err(SetupError::StartupTimeout { unclaimed: slots.unclaimed() });
            }
        }
    }

    let phase_a_ms = setup_start.elapsed().as_millis();
    info!(
        elapsed_ms = phase_a_ms,
        "setup phase A complete — all bots identified"
    );

    let phase_b_start = Instant::now();
    // ── Phase B: Send SetOption + MatchConfig, wait for Ready ───
    for handle in handles.values() {
        if let Some(opts) = setup.bot_options.get(&handle.agent_id) {
            for (name, value) in opts {
                if handle
                    .cmd_tx
                    .send(HostCommand::SetOption {
                        name: name.clone(),
                        value: value.clone(),
                    })
                    .await
                    .is_err()
                {
                    warn!(session = handle.session_id.0, "SetOption send failed");
                }
            }
        }

        let mut cfg = setup.match_config.clone();
        cfg.controlled_players = handle.controlled_players.clone();
        if handle
            .cmd_tx
            .send(HostCommand::MatchConfig(Box::new(cfg)))
            .await
            .is_err()
        {
            warn!(session = handle.session_id.0, "MatchConfig send failed");
        }
    }

    if !all_keys_in(&handles, &ready_set) {
        loop {
            tokio::select! {
                msg = game_rx.recv() => {
                    let Some(msg) = msg else {
                        return Err(SetupError::AllDisconnected);
                    };
                    match msg {
                        SessionMsg::Ready { session_id } => {
                            if handles.contains_key(&session_id) {
                                ready_set.insert(session_id);
                                if all_keys_in(&handles, &ready_set) {
                                    break;
                                }
                            }
                        }
                        SessionMsg::PreprocessingDone { session_id } => {
                            done_set.insert(session_id);
                        }
                        SessionMsg::Disconnected { session_id, reason } => {
                            ready_set.remove(&session_id);
                            if let Some(h) = handles.remove(&session_id) {
                                for &p in &h.controlled_players {
                                    emit(event_tx, MatchEvent::BotDisconnected { player: p, reason });
                                }
                                slots.unreserve(session_id);
                                return Err(SetupError::BotDisconnected {
                                    name: h.name,
                                    agent_id: h.agent_id,
                                });
                            }
                            // Disconnect from an untracked session (e.g. rejected
                            // agent_id) — safe to ignore.
                        }
                        SessionMsg::Connected { session_id, .. } => {
                            debug!(session = session_id.0, "late connection during phase B — ignored");
                        }
                        // Info, Action, etc. — ignored during setup.
                        _ => {}
                    }
                }
                // startup_deadline covers both Phase A and Phase B.
                _ = tokio::time::sleep_until(startup_deadline) => {
                    return Err(SetupError::StartupTimeout { unclaimed: slots.unclaimed() });
                }
            }
        }
    }

    let phase_b_ms = phase_b_start.elapsed().as_millis();
    info!(
        elapsed_ms = phase_b_ms,
        "setup phase B complete — all bots configured"
    );

    let phase_c_start = Instant::now();
    // ── Phase C: StartPreprocessing, wait for PreprocessingDone ───

    // Compute the initial state hash from match_config. Sent to bots via
    // StartPreprocessing so they can use it on preprocessing Info frames.
    // Must match the hash the GUI stores on the root node.
    let initial_state_hash = HashedTurnState::new(OwnedTurnState {
        turn: 0,
        player1_position: setup.match_config.player1_start,
        player2_position: setup.match_config.player2_start,
        player1_score: 0.0,
        player2_score: 0.0,
        player1_mud_turns: 0,
        player2_mud_turns: 0,
        cheese: setup.match_config.cheese.clone(),
        player1_last_move: pyrat::Direction::Stay,
        player2_last_move: pyrat::Direction::Stay,
    })
    .state_hash();

    for handle in handles.values() {
        if handle
            .cmd_tx
            .send(HostCommand::StartPreprocessing {
                state_hash: initial_state_hash,
            })
            .await
            .is_err()
        {
            warn!(
                session = handle.session_id.0,
                "StartPreprocessing send failed"
            );
        }
    }

    emit(event_tx, MatchEvent::PreprocessingStarted);
    info!("preprocessing started");

    // The bot SDK uses the same timeout value to decide when to stop work.
    // Add 500ms margin so PreprocessingDone has time to arrive over TCP.
    let preprocessing_deadline =
        Instant::now() + setup.timing.preprocessing_timeout + Duration::from_millis(500);

    if !all_keys_in(&handles, &done_set) {
        loop {
            tokio::select! {
                msg = game_rx.recv() => {
                    let Some(msg) = msg else {
                        return Err(SetupError::AllDisconnected);
                    };
                    match msg {
                        SessionMsg::PreprocessingDone { session_id } => {
                            if handles.contains_key(&session_id) {
                                done_set.insert(session_id);
                                if all_keys_in(&handles, &done_set) {
                                    break;
                                }
                            }
                        }
                        SessionMsg::Disconnected { session_id, reason } => {
                            if let Some(h) = handles.remove(&session_id) {
                                for &p in &h.controlled_players {
                                    emit(event_tx, MatchEvent::BotDisconnected { player: p, reason });
                                }
                                done_set.remove(&session_id);
                                slots.unreserve(session_id);
                                return Err(SetupError::BotDisconnected {
                                    name: h.name,
                                    agent_id: h.agent_id,
                                });
                            }
                        }
                        SessionMsg::Info { session_id, info } => {
                            if let Some(handle) = handles.get(&session_id) {
                                if let Some(&sender) = handle.controlled_players.first() {
                                    emit(event_tx, MatchEvent::BotInfo {
                                        sender,
                                        turn: info.turn,
                                        state_hash: info.state_hash,
                                        info,
                                    });
                                }
                            }
                        }
                        SessionMsg::Connected { session_id, .. } => {
                            debug!(session = session_id.0, "late connection during phase C — ignored");
                        }
                        // Action, etc. — ignored during setup.
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(preprocessing_deadline) => {
                    let pending: Vec<String> = handles
                        .values()
                        .filter(|h| !done_set.contains(&h.session_id))
                        .map(|h| h.agent_id.clone())
                        .collect();
                    return Err(SetupError::PreprocessingTimeout { pending });
                }
            }
        }
    }

    let phase_c_ms = phase_c_start.elapsed().as_millis();
    info!(
        elapsed_ms = phase_c_ms,
        "setup phase C complete — preprocessing done"
    );

    let total_ms = setup_start.elapsed().as_millis();
    info!(total_ms, "setup complete");

    emit(event_tx, MatchEvent::SetupComplete);
    emit(
        event_tx,
        MatchEvent::MatchStarted {
            config: setup.match_config.clone(),
        },
    );

    let sessions: Vec<SessionHandle> = handles.into_values().collect();
    Ok(SetupResult { sessions })
}

/// Check whether every key in `handles` is present in `set`.
fn all_keys_in(handles: &HashMap<SessionId, SessionHandle>, set: &HashSet<SessionId>) -> bool {
    handles.keys().all(|id| set.contains(id))
}

// ── TCP accept loop ──────────────────────────────────

/// Accept TCP connections and spawn session tasks.
///
/// Runs indefinitely until the `game_tx` channel is closed (receiver dropped).
/// Tests bypass this entirely using duplex pairs.
pub async fn accept_connections(
    listener: TcpListener,
    game_tx: mpsc::Sender<SessionMsg>,
    session_config: SessionConfig,
) {
    let mut next_id: u64 = 1;

    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                warn!(error = %e, "accept failed");
                continue;
            },
        };

        if let Err(e) = stream.set_nodelay(true) {
            warn!(error = %e, "failed to set TCP_NODELAY");
        }

        let session_id = SessionId(next_id);
        next_id += 1;
        info!(session = session_id.0, %addr, "new TCP connection");

        let tx = game_tx.clone();
        let cfg = session_config.clone();

        let span = info_span!(
            "session",
            id = session_id.0,
            %addr,
            agent_id = tracing::field::Empty,
        );
        let (read, write) = tokio::io::split(stream);
        tokio::spawn(
            async move {
                run_session(session_id, read, write, tx, cfg).await;
            }
            .instrument(span),
        );
    }
}
