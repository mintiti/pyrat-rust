use std::collections::{HashMap, HashSet};

use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::session::messages::HostCommand;
use crate::session::{run_session, SessionConfig, SessionId, SessionMsg};
use crate::wire::Player;

use super::config::MatchSetup;
use super::slots::PlayerSlots;

// ── Public types ─────────────────────────────────────

/// Handle to an active session after setup completes.
#[derive(Debug)]
pub struct SessionHandle {
    pub session_id: SessionId,
    pub cmd_tx: mpsc::Sender<HostCommand>,
    pub name: String,
    pub author: String,
    pub agent_id: String,
    pub controlled_players: Vec<Player>,
}

/// Successful result of the setup phase.
#[derive(Debug)]
pub struct SetupResult {
    pub sessions: Vec<SessionHandle>,
}

/// What can go wrong during setup.
#[derive(Debug)]
pub enum SetupError {
    /// Not all player slots were claimed before the startup timeout.
    StartupTimeout,
    /// All connected sessions disconnected before setup completed.
    AllDisconnected,
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
) -> Result<SetupResult, SetupError> {
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
                    SessionMsg::Disconnected { session_id, .. } => {
                        pending.remove(&session_id);
                        if handles.remove(&session_id).is_some() {
                            slots.unreserve(session_id);
                        }
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(startup_deadline) => {
                return Err(SetupError::StartupTimeout);
            }
        }
    }

    // ── Phase B: Send SetOption + MatchConfig, wait for Ready ───
    for handle in handles.values() {
        if let Some(opts) = setup.bot_options.get(&handle.agent_id) {
            for (name, value) in opts {
                let _ = handle
                    .cmd_tx
                    .send(HostCommand::SetOption {
                        name: name.clone(),
                        value: value.clone(),
                    })
                    .await;
            }
        }

        let mut cfg = setup.match_config.clone();
        cfg.controlled_players = handle.controlled_players.clone();
        let _ = handle
            .cmd_tx
            .send(HostCommand::MatchConfig(Box::new(cfg)))
            .await;
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
                        SessionMsg::Disconnected { session_id, .. } => {
                            ready_set.remove(&session_id);
                            if handles.remove(&session_id).is_some() {
                                slots.unreserve(session_id);
                            }
                            if handles.is_empty() {
                                return Err(SetupError::AllDisconnected);
                            }
                        }
                        SessionMsg::Connected { session_id, .. } => {
                            debug!(session = session_id.0, "late connection during phase B — ignored");
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(startup_deadline) => {
                    return Err(SetupError::StartupTimeout);
                }
            }
        }
    }

    // ── Phase C: StartPreprocessing, wait for PreprocessingDone ───
    for handle in handles.values() {
        let _ = handle.cmd_tx.send(HostCommand::StartPreprocessing).await;
    }

    let preprocessing_deadline = Instant::now() + setup.timing.preprocessing_timeout;

    if !all_keys_in(&handles, &done_set) {
        loop {
            tokio::select! {
                msg = game_rx.recv() => {
                    let Some(msg) = msg else {
                        break;
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
                        SessionMsg::Disconnected { session_id, .. } => {
                            handles.remove(&session_id);
                            done_set.remove(&session_id);
                            if handles.is_empty() || all_keys_in(&handles, &done_set) {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(preprocessing_deadline) => {
                    warn!("preprocessing timeout — proceeding with available bots");
                    break;
                }
            }
        }
    }

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

        let session_id = SessionId(next_id);
        next_id += 1;
        info!(session = session_id.0, %addr, "new TCP connection");

        let tx = game_tx.clone();
        let cfg = session_config.clone();

        let (read, write) = tokio::io::split(stream);
        tokio::spawn(async move {
            run_session(session_id, read, write, tx, cfg).await;
        });
    }
}
