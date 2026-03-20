use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use pyrat::game::game_logic::GameState;
use pyrat_host::game_loop::{
    build_owned_match_config, determine_result, run_playing, run_setup, wire_to_engine, MatchEvent,
    MatchSetup, PlayerEntry, PlayingConfig, PlayingState, SetupTiming,
};
use pyrat_host::session::messages::{HostCommand, SessionId, SessionMsg};
use pyrat_host::stub::spawn_stub_bot;
use pyrat_host::wire::{Direction as WireDirection, GameResult, Player, TimingMode};

use tauri_specta::Event;

use crate::commands::{Coord, PlayerState};
use crate::events::{
    BotDisconnectedEvent, BotInfoEvent, Direction as SpectaDirection, MatchOverEvent, MatchWinner,
    PlayerSide, TurnPlayedEvent,
};
use crate::state::AnalysisRx;

pub fn wire_to_specta(d: WireDirection) -> SpectaDirection {
    match d {
        WireDirection::Up => SpectaDirection::Up,
        WireDirection::Right => SpectaDirection::Right,
        WireDirection::Down => SpectaDirection::Down,
        WireDirection::Left => SpectaDirection::Left,
        _ => SpectaDirection::Stay,
    }
}

pub fn specta_to_wire(d: SpectaDirection) -> WireDirection {
    match d {
        SpectaDirection::Up => WireDirection::Up,
        SpectaDirection::Right => WireDirection::Right,
        SpectaDirection::Down => WireDirection::Down,
        SpectaDirection::Left => WireDirection::Left,
        SpectaDirection::Stay => WireDirection::Stay,
    }
}

fn player_side(p: Player) -> PlayerSide {
    if p == Player::Player1 {
        PlayerSide::Player1
    } else {
        PlayerSide::Player2
    }
}

/// Sentinel command value that means "use the built-in random stub bot".
const STUB_SENTINEL: &str = "__random__";

/// Per-player config passed from the command layer.
pub struct PlayerSetup {
    pub command: String,
    pub working_dir: Option<String>,
    pub agent_id: String,
}

/// Run a full match, emitting Tauri events for each phase.
///
/// When `cmd_rx` is `Some`, runs in analysis (step) mode instead of auto-play.
pub async fn run_match(
    app: tauri::AppHandle,
    mut game: GameState,
    players: [PlayerSetup; 2],
    cancel: CancellationToken,
    match_id: u32,
    cmd_rx: Option<AnalysisRx>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let [ref p1, ref p2] = players;
    let p1_is_stub = p1.command == STUB_SENTINEL;
    let p2_is_stub = p2.command == STUB_SENTINEL;

    // Disambiguate agent_ids when both players use the same bot,
    // otherwise the host treats them as a hivemind (one process, both slots).
    let (p1_agent_id, p2_agent_id) = if p1.agent_id == p2.agent_id {
        (format!("{}/1", p1.agent_id), format!("{}/2", p2.agent_id))
    } else {
        (p1.agent_id.clone(), p2.agent_id.clone())
    };

    // 1. Build match config
    let match_config = build_owned_match_config(&game, TimingMode::Wait, 3000, 10000);

    let setup = MatchSetup {
        players: vec![
            PlayerEntry {
                player: Player::Player1,
                agent_id: p1_agent_id.clone(),
            },
            PlayerEntry {
                player: Player::Player2,
                agent_id: p2_agent_id.clone(),
            },
        ],
        match_config,
        bot_options: HashMap::new(),
        timing: SetupTiming {
            startup_timeout: Duration::from_secs(30),
            preprocessing_timeout: Duration::from_secs(10),
        },
    };

    // 2. Create channels
    let (event_tx, event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(64);

    // 3. Spawn stub bots and/or real bot infrastructure
    let mut _stub_handles = Vec::new();
    let mut _accept_handle = None;
    let mut _bot_processes = None;

    let mut next_session_id: u64 = 1;

    if p1_is_stub {
        let sid = SessionId(next_session_id);
        next_session_id += 1;
        _stub_handles.push(spawn_stub_bot(
            sid,
            p1_agent_id.clone(),
            "Random Bot".into(),
            game_tx.clone(),
        ));
    }

    if p2_is_stub {
        let sid = SessionId(next_session_id);
        let _ = next_session_id;
        _stub_handles.push(spawn_stub_bot(
            sid,
            p2_agent_id.clone(),
            "Random Bot".into(),
            game_tx.clone(),
        ));
    }

    if !p1_is_stub || !p2_is_stub {
        use pyrat_host::game_loop::{accept_connections, launch_bots, BotConfig};
        use pyrat_host::session::SessionConfig;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        info!(port, "listening for bot connections");

        let default_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut bot_configs = Vec::new();

        if !p1_is_stub {
            bot_configs.push(BotConfig {
                run_command: p1.command.clone(),
                working_dir: p1
                    .working_dir
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| default_cwd.clone()),
                agent_id: p1_agent_id.clone(),
            });
        }
        if !p2_is_stub {
            bot_configs.push(BotConfig {
                run_command: p2.command.clone(),
                working_dir: p2
                    .working_dir
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or(default_cwd),
                agent_id: p2_agent_id,
            });
        }

        _bot_processes = Some(launch_bots(&bot_configs, port)?);
        let session_config = SessionConfig::default();
        _accept_handle = Some(tokio::spawn(accept_connections(
            listener,
            game_tx.clone(),
            session_config,
        )));
    }

    // 4. Spawn event forwarder — converts MatchEvents to Tauri events
    let forwarder = tokio::spawn(forward_events(event_rx, app.clone(), match_id));

    // 5. Run match logic with cancellation support
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("Match cancelled".into()),
        r = async {
            match cmd_rx {
                Some(mut rx) => run_analysis_inner(&mut game, &setup, &mut game_rx, &event_tx, &mut rx).await,
                None => run_match_inner(&mut game, &setup, &mut game_rx, &event_tx).await,
            }
        } => r,
    };

    // 6. Cleanup — runs on both cancel and normal completion
    if let Some(h) = _accept_handle {
        h.abort();
    }
    for h in _stub_handles {
        h.abort();
    }
    drop(event_tx);
    let _ = forwarder.await;

    result
}

/// The actual match logic (setup + playing + shutdown), separated so
/// `tokio::select!` can race it against cancellation.
async fn run_match_inner(
    game: &mut GameState,
    setup: &MatchSetup,
    game_rx: &mut mpsc::Receiver<SessionMsg>,
    event_tx: &mpsc::UnboundedSender<MatchEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let setup_result = run_setup(setup, game_rx, Some(event_tx)).await?;

    let playing_config = PlayingConfig {
        move_timeout: Duration::from_secs(3),
    };
    let _match_result = run_playing(
        game,
        &setup_result.sessions,
        game_rx,
        &playing_config,
        Some(event_tx),
    )
    .await?;

    shutdown_sessions(&setup_result.sessions, game_rx).await;
    Ok(())
}

// ── Analysis (step) mode ────────────────────────────

use crate::state::{AnalysisCmd, AnalysisResp};
use pyrat_host::game_loop::SessionHandle;

enum AnalysisPhase {
    Idle,
    Collecting {
        turn: u16,
        p1_action: Option<WireDirection>,
        p2_action: Option<WireDirection>,
        deadline: Option<tokio::time::Instant>,
    },
}

/// Analysis mode: setup then enter command loop for turn-by-turn control.
async fn run_analysis_inner(
    game: &mut GameState,
    setup: &MatchSetup,
    game_rx: &mut mpsc::Receiver<SessionMsg>,
    event_tx: &mpsc::UnboundedSender<MatchEvent>,
    cmd_rx: &mut AnalysisRx,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let setup_result = run_setup(setup, game_rx, Some(event_tx)).await?;
    let sessions = &setup_result.sessions;
    let mut playing = PlayingState::new(sessions);
    let mut phase = AnalysisPhase::Idle;

    loop {
        let (collecting, has_deadline, deadline) = match &phase {
            AnalysisPhase::Idle => (false, false, tokio::time::Instant::now()),
            AnalysisPhase::Collecting { deadline, .. } => {
                let has = deadline.is_some();
                let dl = deadline
                    .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(86400));
                (true, has, dl)
            },
        };

        tokio::select! {
            biased;

            cmd = cmd_rx.recv() => {
                let Some((cmd, reply)) = cmd else { break; };
                match cmd {
                    AnalysisCmd::StartTurn { duration_ms } => {
                        // If collecting, stop + drain first
                        if collecting {
                            send_stop(sessions, &mut playing).await;
                            drain_rx(game_rx);
                        }

                        // Build and send turn state to all connected sessions
                        let turn_state = playing.build_turn_state(game);
                        send_turn_state(sessions, &mut playing, &turn_state).await;

                        let dl = if duration_ms > 0 {
                            Some(tokio::time::Instant::now() + Duration::from_millis(duration_ms))
                        } else {
                            None
                        };

                        phase = AnalysisPhase::Collecting {
                            turn: game.turn,
                            p1_action: None,
                            p2_action: None,
                            deadline: dl,
                        };
                        let _ = reply.send(AnalysisResp::TurnStarted);
                    }

                    AnalysisCmd::StopTurn => {
                        let (p1, p2) = finish_collecting(
                            sessions, &mut playing, game_rx, &mut phase, event_tx,
                        ).await;
                        let _ = reply.send(AnalysisResp::Actions { p1, p2 });
                    }

                    AnalysisCmd::Advance { actions } => {
                        // Stop + collect if currently collecting
                        let (collected_p1, collected_p2) = if collecting {
                            finish_collecting(
                                sessions, &mut playing, game_rx, &mut phase, event_tx,
                            ).await
                        } else {
                            (WireDirection::Stay, WireDirection::Stay)
                        };

                        // Provided actions override collected ones
                        let (p1, p2) = if let Some([a1, a2]) = actions {
                            (a1, a2)
                        } else {
                            (collected_p1, collected_p2)
                        };

                        // Step the engine
                        let result = game.process_turn(wire_to_engine(p1), wire_to_engine(p2));
                        playing.record_actions(p1, p2);

                        // Emit TurnPlayed
                        let turn_state = playing.build_turn_state(game);
                        emit_event(event_tx, MatchEvent::TurnPlayed {
                            state: turn_state.clone(),
                            p1_action: p1,
                            p2_action: p2,
                        });

                        if result.game_over {
                            let match_result = determine_result(game);
                            send_game_over(sessions, &mut playing, &match_result).await;
                            emit_event(event_tx, MatchEvent::MatchOver {
                                result: match_result,
                            });
                            let _ = reply.send(AnalysisResp::Advanced {
                                p1, p2, game_over: true,
                            });
                            break;
                        }

                        phase = AnalysisPhase::Idle;
                        let _ = reply.send(AnalysisResp::Advanced {
                            p1, p2, game_over: false,
                        });
                    }
                }
            }

            msg = game_rx.recv(), if collecting => {
                let Some(msg) = msg else { break; };
                handle_bot_msg(msg, &mut phase, &mut playing, event_tx);
            }

            _ = tokio::time::sleep_until(deadline), if collecting && has_deadline => {
                // Deadline hit: send Stop to all sessions, remove deadline
                send_stop(sessions, &mut playing).await;
                if let AnalysisPhase::Collecting { deadline: ref mut dl, .. } = phase {
                    *dl = None;
                }
            }
        }
    }

    shutdown_sessions(sessions, game_rx).await;
    Ok(())
}

/// Send HostCommand::Stop to all connected sessions.
async fn send_stop(sessions: &[SessionHandle], playing: &mut PlayingState) {
    for s in sessions {
        if !playing.disconnected().contains(&s.session_id)
            && s.cmd_tx.send(HostCommand::Stop).await.is_err()
        {
            playing.disconnected_mut().insert(s.session_id);
        }
    }
}

/// Send a TurnState to all connected sessions.
async fn send_turn_state(
    sessions: &[SessionHandle],
    playing: &mut PlayingState,
    turn_state: &pyrat_host::game_loop::OwnedTurnState,
) {
    for s in sessions {
        if !playing.disconnected().contains(&s.session_id)
            && s.cmd_tx
                .send(HostCommand::TurnState(Box::new(turn_state.clone())))
                .await
                .is_err()
        {
            playing.disconnected_mut().insert(s.session_id);
        }
    }
}

/// Send GameOver to all connected sessions.
async fn send_game_over(
    sessions: &[SessionHandle],
    playing: &mut PlayingState,
    result: &pyrat_host::game_loop::MatchResult,
) {
    for s in sessions {
        if !playing.disconnected().contains(&s.session_id)
            && s.cmd_tx
                .send(HostCommand::GameOver {
                    result: result.result,
                    player1_score: result.player1_score,
                    player2_score: result.player2_score,
                })
                .await
                .is_err()
        {
            playing.disconnected_mut().insert(s.session_id);
        }
    }
}

/// Non-blocking drain of game_rx (discard all pending messages).
fn drain_rx(game_rx: &mut mpsc::Receiver<SessionMsg>) {
    while game_rx.try_recv().is_ok() {}
}

/// Stop bots, collect both actions (with 2s safety timeout), transition to Idle.
async fn finish_collecting(
    sessions: &[SessionHandle],
    playing: &mut PlayingState,
    game_rx: &mut mpsc::Receiver<SessionMsg>,
    phase: &mut AnalysisPhase,
    event_tx: &mpsc::UnboundedSender<MatchEvent>,
) -> (WireDirection, WireDirection) {
    // Extract current action slots and turn
    let (current_turn, mut p1, mut p2) = match phase {
        AnalysisPhase::Collecting {
            turn,
            p1_action,
            p2_action,
            ..
        } => (*turn, *p1_action, *p2_action),
        AnalysisPhase::Idle => {
            return (WireDirection::Stay, WireDirection::Stay);
        },
    };

    // Pre-fill Stay for disconnected players
    for sid in playing.disconnected().clone() {
        if let Some(players) = playing.session_players().get(&sid) {
            for &player in players {
                fill_action(player, WireDirection::Stay, &mut p1, &mut p2);
            }
        }
    }

    if let (Some(a1), Some(a2)) = (p1, p2) {
        *phase = AnalysisPhase::Idle;
        return (a1, a2);
    }

    // Send Stop to prompt bots to commit
    send_stop(sessions, playing).await;

    // Drain until both actions arrive (2s safety timeout)
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if p1.is_some() && p2.is_some() {
            break;
        }
        tokio::select! {
            msg = game_rx.recv() => {
                let Some(msg) = msg else { break; };
                match msg {
                    SessionMsg::Action { player, direction, .. } => {
                        fill_action(player, direction, &mut p1, &mut p2);
                    }
                    SessionMsg::Info { session_id, info } => {
                        if let Some(players) = playing.session_players().get(&session_id) {
                            if let Some(&sender) = players.first() {
                                emit_event(event_tx, MatchEvent::BotInfo {
                                    sender,
                                    turn: current_turn,
                                    info,
                                });
                            }
                        }
                    }
                    SessionMsg::Disconnected { session_id, reason } => {
                        playing.disconnected_mut().insert(session_id);
                        if let Some(players) = playing.session_players().get(&session_id) {
                            for &player in players {
                                fill_action(player, WireDirection::Stay, &mut p1, &mut p2);
                                emit_event(event_tx, MatchEvent::BotDisconnected { player, reason });
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                debug!("finish_collecting safety timeout — filling missing with Stay");
                break;
            }
        }
    }

    *phase = AnalysisPhase::Idle;
    (
        p1.unwrap_or(WireDirection::Stay),
        p2.unwrap_or(WireDirection::Stay),
    )
}

/// Handle a bot message during the Collecting phase.
fn handle_bot_msg(
    msg: SessionMsg,
    phase: &mut AnalysisPhase,
    playing: &mut PlayingState,
    event_tx: &mpsc::UnboundedSender<MatchEvent>,
) {
    let (current_turn, p1_action, p2_action) = match phase {
        AnalysisPhase::Collecting {
            turn,
            p1_action,
            p2_action,
            ..
        } => (*turn, p1_action, p2_action),
        _ => return,
    };

    match msg {
        SessionMsg::Action {
            player,
            direction,
            turn,
            ..
        } => {
            if turn != current_turn {
                debug!(turn, current_turn, "stale action ignored in analysis");
                return;
            }
            fill_action(player, direction, p1_action, p2_action);
        },
        SessionMsg::Info { session_id, info } => {
            if let Some(players) = playing.session_players().get(&session_id) {
                if let Some(&sender) = players.first() {
                    emit_event(
                        event_tx,
                        MatchEvent::BotInfo {
                            sender,
                            turn: current_turn,
                            info,
                        },
                    );
                }
            }
        },
        SessionMsg::Disconnected {
            session_id, reason, ..
        } => {
            debug!(
                session = session_id.0,
                ?reason,
                "session disconnected during analysis"
            );
            playing.disconnected_mut().insert(session_id);
            if let Some(players) = playing.session_players().get(&session_id) {
                for &player in players {
                    fill_action(player, WireDirection::Stay, p1_action, p2_action);
                    emit_event(event_tx, MatchEvent::BotDisconnected { player, reason });
                }
            }
        },
        _ => {},
    }
}

/// Insert a direction for the given player, first-wins.
fn fill_action(
    player: Player,
    direction: WireDirection,
    p1: &mut Option<WireDirection>,
    p2: &mut Option<WireDirection>,
) {
    match player {
        Player::Player1 => {
            if p1.is_none() {
                *p1 = Some(direction);
            }
        },
        Player::Player2 => {
            if p2.is_none() {
                *p2 = Some(direction);
            }
        },
        _ => {
            warn!(player = player.0, "unknown player in action");
        },
    }
}

fn emit_event(tx: &mpsc::UnboundedSender<MatchEvent>, event: MatchEvent) {
    if tx.send(event).is_err() {
        warn!("event receiver dropped — event lost");
    }
}

// ── Shared helpers ──────────────────────────────────

/// Send Shutdown to all sessions and drain disconnects with 2s timeout.
async fn shutdown_sessions(sessions: &[SessionHandle], game_rx: &mut mpsc::Receiver<SessionMsg>) {
    let session_count = sessions.len();
    for s in sessions {
        let _ = s.cmd_tx.send(HostCommand::Shutdown).await;
    }

    let mut disconnected = 0usize;
    let drain_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while disconnected < session_count {
        tokio::select! {
            msg = game_rx.recv() => {
                match msg {
                    Some(SessionMsg::Disconnected { .. }) => { disconnected += 1; }
                    Some(_) => {}
                    None => break,
                }
            }
            _ = tokio::time::sleep_until(drain_deadline) => {
                warn!(
                    remaining = session_count - disconnected,
                    "shutdown drain timed out"
                );
                break;
            }
        }
    }
}

/// Receives MatchEvents from the host and emits them as Tauri events.
async fn forward_events(
    mut event_rx: mpsc::UnboundedReceiver<MatchEvent>,
    app: tauri::AppHandle,
    match_id: u32,
) {
    while let Some(event) = event_rx.recv().await {
        match event {
            MatchEvent::TurnPlayed {
                state,
                p1_action,
                p2_action,
            } => {
                let payload = TurnPlayedEvent {
                    match_id,
                    turn: state.turn,
                    player1: PlayerState {
                        position: state.player1_position.into(),
                        score: state.player1_score,
                        mud_turns: state.player1_mud_turns,
                    },
                    player2: PlayerState {
                        position: state.player2_position.into(),
                        score: state.player2_score,
                        mud_turns: state.player2_mud_turns,
                    },
                    cheese: state.cheese.iter().copied().map(Coord::from).collect(),
                    player1_action: wire_to_specta(p1_action),
                    player2_action: wire_to_specta(p2_action),
                };
                let _ = payload.emit(&app);
            },
            MatchEvent::BotDisconnected { player, reason } => {
                let _ = BotDisconnectedEvent {
                    match_id,
                    player: player_side(player),
                    reason: format!("{reason:?}"),
                }
                .emit(&app);
            },
            MatchEvent::MatchOver { result } => {
                let winner = if result.result == GameResult::Player1 {
                    MatchWinner::Player1
                } else if result.result == GameResult::Player2 {
                    MatchWinner::Player2
                } else if result.result == GameResult::Draw {
                    MatchWinner::Draw
                } else {
                    warn!(result = ?result.result, "unexpected GameResult variant");
                    MatchWinner::Draw
                };
                let _ = MatchOverEvent {
                    match_id,
                    winner,
                    player1_score: result.player1_score,
                    player2_score: result.player2_score,
                    turns_played: result.turns_played,
                }
                .emit(&app);
            },
            MatchEvent::BotInfo { sender, turn, info } => {
                let _ = BotInfoEvent {
                    match_id,
                    sender: player_side(sender),
                    subject: player_side(info.player),
                    turn,
                    multipv: info.multipv,
                    target: info.target.map(Coord::from),
                    depth: info.depth,
                    nodes: info.nodes,
                    score: info.score,
                    pv: info.pv.iter().map(|&d| wire_to_specta(d)).collect(),
                    message: info.message,
                }
                .emit(&app);
            },
            _ => {},
        }
    }
}
