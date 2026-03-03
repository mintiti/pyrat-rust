use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use pyrat::game::game_logic::GameState;
use pyrat_host::game_loop::{
    build_owned_match_config, run_playing, run_setup, MatchEvent, MatchSetup, PlayerEntry,
    PlayingConfig, SetupTiming,
};
use pyrat_host::session::messages::{SessionId, SessionMsg};
use pyrat_host::stub::spawn_stub_bot;
use pyrat_host::wire::{Direction as WireDirection, GameResult, Player, TimingMode};

use tauri_specta::Event;

use crate::commands::{Coord, PlayerState};
use crate::events::{
    BotDisconnectedEvent, BotInfoEvent, Direction as SpectaDirection, MatchOverEvent, MatchWinner,
    TurnPlayedEvent,
};

fn wire_to_specta(d: WireDirection) -> SpectaDirection {
    match d {
        WireDirection::Up => SpectaDirection::Up,
        WireDirection::Right => SpectaDirection::Right,
        WireDirection::Down => SpectaDirection::Down,
        WireDirection::Left => SpectaDirection::Left,
        _ => SpectaDirection::Stay,
    }
}

/// Sentinel command value that means "use the built-in random stub bot".
const STUB_SENTINEL: &str = "__random__";

/// Run a full match, emitting Tauri events for each phase.
///
/// Follows the same pattern as `headless/src/main.rs`.
/// Accepts a `CancellationToken` for cooperative shutdown.
pub async fn run_match(
    app: tauri::AppHandle,
    mut game: GameState,
    player1_cmd: String,
    player2_cmd: String,
    cancel: CancellationToken,
    match_id: u32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let p1_is_stub = player1_cmd == STUB_SENTINEL;
    let p2_is_stub = player2_cmd == STUB_SENTINEL;

    // 1. Build match config
    let match_config = build_owned_match_config(&game, TimingMode::Wait, 3000, 10000);

    let setup = MatchSetup {
        players: vec![
            PlayerEntry {
                player: Player::Player1,
                agent_id: "player1".into(),
            },
            PlayerEntry {
                player: Player::Player2,
                agent_id: "player2".into(),
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
    // These are Option-wrapped so we can skip TCP when both players are stubs.
    let mut _accept_handle = None;
    let mut _bot_processes = None;

    let mut next_session_id: u64 = 1;

    if p1_is_stub {
        let sid = SessionId(next_session_id);
        next_session_id += 1;
        _stub_handles.push(spawn_stub_bot(
            sid,
            "player1".into(),
            "Random Bot".into(),
            game_tx.clone(),
        ));
    }

    if p2_is_stub {
        let sid = SessionId(next_session_id);
        // next_session_id += 1; // last assignment — suppress unused warning
        let _ = next_session_id;
        _stub_handles.push(spawn_stub_bot(
            sid,
            "player2".into(),
            "Random Bot".into(),
            game_tx.clone(),
        ));
    }

    // Only set up TCP + subprocesses if at least one player is real.
    if !p1_is_stub || !p2_is_stub {
        use pyrat_host::game_loop::{accept_connections, launch_bots, BotConfig};
        use pyrat_host::session::SessionConfig;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        info!(port, "listening for bot connections");

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut bot_configs = Vec::new();

        if !p1_is_stub {
            bot_configs.push(BotConfig {
                run_command: player1_cmd,
                working_dir: cwd.clone(),
                agent_id: "player1".into(),
            });
        }
        if !p2_is_stub {
            bot_configs.push(BotConfig {
                run_command: player2_cmd,
                working_dir: cwd,
                agent_id: "player2".into(),
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
        r = run_match_inner(&mut game, &setup, &mut game_rx, &event_tx) => r,
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
    // Run setup
    let setup_result = run_setup(setup, game_rx, Some(event_tx)).await?;

    // Run playing
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

    // Shutdown sessions and drain disconnects
    let session_count = setup_result.sessions.len();
    for s in &setup_result.sessions {
        let _ = s
            .cmd_tx
            .send(pyrat_host::session::messages::HostCommand::Shutdown)
            .await;
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

    Ok(())
}

/// Receives MatchEvents from the host and emits them as Tauri events.
/// No pacing delay — the frontend handles playback speed.
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
                        position: Coord {
                            x: state.player1_position.0,
                            y: state.player1_position.1,
                        },
                        score: state.player1_score,
                    },
                    player2: PlayerState {
                        position: Coord {
                            x: state.player2_position.0,
                            y: state.player2_position.1,
                        },
                        score: state.player2_score,
                    },
                    cheese: state.cheese.iter().map(|&(x, y)| Coord { x, y }).collect(),
                    player1_action: wire_to_specta(p1_action),
                    player2_action: wire_to_specta(p2_action),
                };
                let _ = payload.emit(&app);
            },
            MatchEvent::BotDisconnected { player, reason } => {
                let player_name = if player == Player::Player1 {
                    "Rat"
                } else {
                    "Python"
                };
                let _ = BotDisconnectedEvent {
                    match_id,
                    player: player_name.to_string(),
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
            MatchEvent::BotInfo { player, turn, info } => {
                let player_str = if player == Player::Player1 {
                    "player1"
                } else {
                    "player2"
                };
                let _ = BotInfoEvent {
                    match_id,
                    player: player_str.into(),
                    turn,
                    target: info.target.map(|(x, y)| Coord { x, y }),
                    depth: info.depth,
                    nodes: info.nodes,
                    score: info.score,
                    path: info.path.iter().map(|&(x, y)| Coord { x, y }).collect(),
                    message: info.message,
                }
                .emit(&app);
            },
            _ => {},
        }
    }
}
