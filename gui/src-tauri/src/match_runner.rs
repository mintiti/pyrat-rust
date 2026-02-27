use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{info, warn};

use pyrat::game::game_logic::GameState;
use pyrat_host::game_loop::{
    accept_connections, build_owned_match_config, launch_bots, run_playing, run_setup, BotConfig,
    MatchEvent, MatchSetup, PlayerEntry, PlayingConfig, SetupTiming,
};
use pyrat_host::session::messages::SessionMsg;
use pyrat_host::session::SessionConfig;
use pyrat_host::wire::{GameResult, Player, TimingMode};

use tauri_specta::Event;

use crate::commands::{Coord, PlayerState};
use crate::events::{MatchOverEvent, MatchWinner, TurnPlayedEvent};

/// Run a full match, emitting Tauri events for each phase.
///
/// Follows the same pattern as `headless/src/main.rs`.
pub async fn run_match(
    app: tauri::AppHandle,
    mut game: GameState,
    player1_cmd: String,
    player2_cmd: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Build match config
    let match_config = build_owned_match_config(&game, TimingMode::Wait, 3000, 10000);

    // 2. Build setup
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let bot_configs = vec![
        BotConfig {
            run_command: player1_cmd,
            working_dir: cwd.clone(),
            agent_id: "player1".into(),
        },
        BotConfig {
            run_command: player2_cmd,
            working_dir: cwd,
            agent_id: "player2".into(),
        },
    ];

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

    // 3. Bind TCP listener
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    info!(port, "listening for bot connections");

    // 4. Create event channel
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

    // 5. Launch bots
    let _bot_processes = launch_bots(&bot_configs, port)?;

    // 6. Accept connections
    let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(64);
    let session_config = SessionConfig::default();
    tokio::spawn(accept_connections(listener, game_tx, session_config));

    // 7. Spawn event forwarder — converts MatchEvents to Tauri events
    let forwarder_app = app.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                MatchEvent::TurnPlayed { state, .. } => {
                    let payload = TurnPlayedEvent {
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
                    };
                    let _ = payload.emit(&forwarder_app);
                    // Delay for watchability
                    tokio::time::sleep(Duration::from_millis(200)).await;
                },
                MatchEvent::MatchOver { result } => {
                    let winner = match result.result {
                        x if x == GameResult::Player1 => MatchWinner::Player1,
                        x if x == GameResult::Player2 => MatchWinner::Player2,
                        _ => MatchWinner::Draw,
                    };
                    let _ = MatchOverEvent {
                        winner,
                        player1_score: result.player1_score,
                        player2_score: result.player2_score,
                        turns_played: result.turns_played,
                    }
                    .emit(&forwarder_app);
                },
                _ => {},
            }
        }
    });

    // 8. Run setup
    let setup_result = run_setup(&setup, &mut game_rx, Some(&event_tx)).await?;

    // 9. Run playing
    let playing_config = PlayingConfig {
        move_timeout: Duration::from_secs(3),
    };
    let _match_result = run_playing(
        &mut game,
        &setup_result.sessions,
        &mut game_rx,
        &playing_config,
        Some(&event_tx),
    )
    .await?;

    // 10. Shutdown sessions and drain disconnects
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

    // 11. Drop event_tx to close forwarder
    drop(event_tx);
    let _ = forwarder.await;

    Ok(())
}
