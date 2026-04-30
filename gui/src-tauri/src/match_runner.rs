use std::path::PathBuf;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use pyrat::game::game_logic::GameState;
use pyrat::{Coordinates, Direction as EngineDirection};

use pyrat_host::launch::{launch_bots, BotConfig};
use pyrat_host::match_config::build_match_config;
use pyrat_host::match_host::{
    Collected, Match, MatchError, MatchEvent, Playing, PlayingConfig, SetupTiming, StepResult,
    Thinking,
};
use pyrat_host::player::{
    accept_players, EmbeddedPlayer, EventSink, Player as PlayerTrait, PlayerIdentity,
};
use pyrat_host::wire::{GameResult, Player, TimingMode};
use pyrat_protocol::TurnState;

use tauri_specta::Event;

use crate::commands::{AnalysisPosition, Coord, PlayerState};
use crate::events::{
    BotDisconnectedEvent, BotInfoEvent, Direction as SpectaDirection, MatchOverEvent, MatchWinner,
    PlayerSide, PreprocessingStartedEvent, SetupCompleteEvent, TurnPlayedEvent,
};
use crate::random_bot::RandomBot;
use crate::state::{AnalysisCmd, AnalysisResp, AnalysisRx};

const STUB_SENTINEL: &str = "__random__";
const MOVE_TIMEOUT_MS: u32 = 3000;
const PREPROCESSING_TIMEOUT_MS: u32 = 10000;
const STARTUP_TIMEOUT_MS: u64 = 30000;

pub fn engine_to_specta(d: EngineDirection) -> SpectaDirection {
    match d {
        EngineDirection::Up => SpectaDirection::Up,
        EngineDirection::Right => SpectaDirection::Right,
        EngineDirection::Down => SpectaDirection::Down,
        EngineDirection::Left => SpectaDirection::Left,
        EngineDirection::Stay => SpectaDirection::Stay,
    }
}

pub fn specta_to_engine(d: SpectaDirection) -> EngineDirection {
    match d {
        SpectaDirection::Up => EngineDirection::Up,
        SpectaDirection::Right => EngineDirection::Right,
        SpectaDirection::Down => EngineDirection::Down,
        SpectaDirection::Left => EngineDirection::Left,
        SpectaDirection::Stay => EngineDirection::Stay,
    }
}

fn player_side(p: Player) -> PlayerSide {
    if p == Player::Player1 {
        PlayerSide::Player1
    } else {
        PlayerSide::Player2
    }
}

/// Per-player config passed from the command layer.
pub struct PlayerSetup {
    pub command: String,
    pub working_dir: Option<String>,
    pub agent_id: String,
    pub options: Vec<(String, String)>,
}

/// Run a match, emitting Tauri events for each phase.
///
/// `cmd_rx = None` runs auto-mode (`Match::run`). `cmd_rx = Some` enters
/// analysis-mode and dispatches per-command typestate transitions.
pub async fn run_match(
    app: tauri::AppHandle,
    game: GameState,
    players: [PlayerSetup; 2],
    cancel: CancellationToken,
    match_id: u32,
    cmd_rx: Option<AnalysisRx>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let p1_is_stub = players[0].command == STUB_SENTINEL;
    let p2_is_stub = players[1].command == STUB_SENTINEL;

    // Disambiguate agent_ids when both slots use the same bot — accept_players
    // rejects duplicate agent_ids as HivemindNotSupported.
    let (p1_agent_id, p2_agent_id) = if players[0].agent_id == players[1].agent_id {
        (
            format!("{}/1", players[0].agent_id),
            format!("{}/2", players[1].agent_id),
        )
    } else {
        (players[0].agent_id.clone(), players[1].agent_id.clone())
    };

    let match_config = build_match_config(
        &game,
        TimingMode::Wait,
        MOVE_TIMEOUT_MS,
        PREPROCESSING_TIMEOUT_MS,
    );

    let (event_tx, event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let event_sink = EventSink::new(event_tx.clone());

    // Bind listener + launch real bots (if any), accept TCP players first; then
    // fill remaining slots with EmbeddedPlayer<RandomBot>. `_bot_processes`
    // owns child handles for the rest of the function so the bots stay alive.
    let mut _bot_processes = None;
    let mut tcp_slots: [Option<Box<dyn PlayerTrait>>; 2] = [None, None];

    if !p1_is_stub || !p2_is_stub {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        info!(port, "listening for bot connections");

        let default_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut bot_configs = Vec::new();
        let mut expected = Vec::new();

        if !p1_is_stub {
            bot_configs.push(BotConfig {
                run_command: players[0].command.clone(),
                working_dir: players[0]
                    .working_dir
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| default_cwd.clone()),
                agent_id: p1_agent_id.clone(),
            });
            expected.push((Player::Player1, p1_agent_id.clone()));
        }
        if !p2_is_stub {
            bot_configs.push(BotConfig {
                run_command: players[1].command.clone(),
                working_dir: players[1]
                    .working_dir
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or(default_cwd),
                agent_id: p2_agent_id.clone(),
            });
            expected.push((Player::Player2, p2_agent_id.clone()));
        }

        let mut launched = launch_bots(&bot_configs, port)?;

        // Drain bot stderr so the OS pipe buffer doesn't fill and block the
        // bot mid-write, and so we actually see panics / SDK errors. Mirrors
        // the pattern in headless main.rs.
        for (agent_id, stderr) in launched.take_stderr_handles() {
            tokio::task::spawn_blocking(move || {
                use std::io::BufRead;
                let reader = std::io::BufReader::new(stderr);
                let mut count = 0usize;
                const MAX_LINES: usize = 200;
                for line in reader.lines() {
                    match line {
                        Ok(text) if count < MAX_LINES => {
                            warn!(%agent_id, "{text}");
                            count += 1;
                        },
                        Ok(_) if count == MAX_LINES => {
                            warn!(%agent_id, "stderr truncated after {MAX_LINES} lines");
                            count += 1;
                        },
                        Ok(_) => {},
                        Err(_) => break,
                    }
                }
            });
        }
        launched.start_exit_monitor(tracing::Span::current());
        _bot_processes = Some(launched);

        let accepted = accept_players(
            &listener,
            &expected,
            event_sink.clone(),
            Duration::from_millis(STARTUP_TIMEOUT_MS),
        )
        .await?;
        let [a, b] = accepted;
        if let Some(t) = a {
            tcp_slots[0] = Some(Box::new(t) as Box<dyn PlayerTrait>);
        }
        if let Some(t) = b {
            tcp_slots[1] = Some(Box::new(t) as Box<dyn PlayerTrait>);
        }
    }

    let mut slots: [Option<Box<dyn PlayerTrait>>; 2] = [None, None];
    for (i, slot, agent_id, is_stub) in [
        (0usize, Player::Player1, p1_agent_id.clone(), p1_is_stub),
        (1usize, Player::Player2, p2_agent_id.clone(), p2_is_stub),
    ] {
        if is_stub {
            let identity = PlayerIdentity {
                name: "Random Bot".into(),
                author: "stub".into(),
                agent_id,
                slot,
            };
            let p = EmbeddedPlayer::accept(RandomBot, identity, event_sink.clone()).await?;
            slots[i] = Some(Box::new(p) as Box<dyn PlayerTrait>);
        } else {
            slots[i] = tcp_slots[i].take();
        }
    }
    let p1 = slots[0].take().ok_or("missing player1")?;
    let p2 = slots[1].take().ok_or("missing player2")?;

    let forwarder = tokio::spawn(forward_events(event_rx, app.clone(), match_id));

    let p1_opts = players[0].options.clone();
    let p2_opts = players[1].options.clone();

    let m = Match::new(
        game,
        [p1, p2],
        match_config,
        [p1_opts, p2_opts],
        SetupTiming {
            configure_timeout: Duration::from_secs(5),
            preprocessing_timeout: Duration::from_millis(u64::from(PREPROCESSING_TIMEOUT_MS)),
        },
        PlayingConfig {
            move_timeout: Duration::from_millis(u64::from(MOVE_TIMEOUT_MS)),
            ..PlayingConfig::default()
        },
        Some(event_tx.clone()),
    );

    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err::<(), Box<dyn std::error::Error + Send + Sync>>("Match cancelled".into()),
        r = async {
            match cmd_rx {
                None => m.run().await.map(|_| ()).map_err(boxed),
                Some(rx) => run_analysis(m, rx).await,
            }
        } => r,
    };

    drop(event_tx);
    let _ = forwarder.await;
    drop(_bot_processes);
    result
}

fn boxed(e: MatchError) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(e) as Box<dyn std::error::Error + Send + Sync>
}

// ── Analysis (step) mode ────────────────────────────

enum AnalysisState {
    Playing(Match<Playing>),
    Thinking(Match<Thinking>),
    Collected(Match<Collected>),
    Finished,
}

async fn run_analysis(
    m: Match<pyrat_host::match_host::Created>,
    mut cmd_rx: AnalysisRx,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ready = m.setup().await.map_err(boxed)?;
    let mut state = AnalysisState::Playing(ready.start());

    while let Some((cmd, reply)) = cmd_rx.recv().await {
        state = step_analysis(state, cmd, reply).await?;
        if matches!(state, AnalysisState::Finished) {
            break;
        }
    }

    // Caller closed the channel without reaching GameOver. Drop runs Player
    // close on Drop; Thinking gets a graceful stop_and_collect first so
    // bot dispatchers see Stop and don't dangle.
    if let AnalysisState::Thinking(t) = state {
        let _ = t.stop_and_collect().await;
    }
    Ok(())
}

async fn step_analysis(
    state: AnalysisState,
    cmd: AnalysisCmd,
    reply: oneshot::Sender<AnalysisResp>,
) -> Result<AnalysisState, Box<dyn std::error::Error + Send + Sync>> {
    match (state, cmd) {
        // ── StartTurn ─────────────────────────────────
        (AnalysisState::Playing(p), AnalysisCmd::StartTurn { position }) => {
            let next = start_turn(p, position).await?;
            let _ = reply.send(AnalysisResp::TurnStarted);
            Ok(AnalysisState::Thinking(next))
        },
        (AnalysisState::Thinking(t), AnalysisCmd::StartTurn { position }) => {
            // Abort the in-flight turn (stop bots, drop their actions), restart.
            let collected = t.stop_and_collect().await.map_err(boxed)?;
            let p = collected.discard();
            let next = start_turn(p, position).await?;
            let _ = reply.send(AnalysisResp::TurnStarted);
            Ok(AnalysisState::Thinking(next))
        },
        (AnalysisState::Collected(c), AnalysisCmd::StartTurn { position }) => {
            let p = c.discard();
            let next = start_turn(p, position).await?;
            let _ = reply.send(AnalysisResp::TurnStarted);
            Ok(AnalysisState::Thinking(next))
        },

        // ── StopTurn ──────────────────────────────────
        (AnalysisState::Thinking(t), AnalysisCmd::StopTurn) => {
            let collected = t.stop_and_collect().await.map_err(boxed)?;
            let (p1, p2) = outcome_directions(collected.outcomes());
            let _ = reply.send(AnalysisResp::Actions { p1, p2 });
            Ok(AnalysisState::Collected(collected))
        },
        (state, AnalysisCmd::StopTurn) => {
            // Not in Thinking — no actions to collect. Mirror legacy's permissive
            // behavior with Stay/Stay rather than erroring out.
            let _ = reply.send(AnalysisResp::Actions {
                p1: EngineDirection::Stay,
                p2: EngineDirection::Stay,
            });
            Ok(state)
        },

        // ── Advance ───────────────────────────────────
        (AnalysisState::Collected(c), AnalysisCmd::Advance { actions }) => {
            advance_collected(c, actions, reply).await
        },
        (AnalysisState::Thinking(t), AnalysisCmd::Advance { actions }) => {
            // Implicit stop_and_collect, then advance.
            let collected = t.stop_and_collect().await.map_err(boxed)?;
            advance_collected(collected, actions, reply).await
        },
        (AnalysisState::Playing(p), AnalysisCmd::Advance { actions }) => {
            // Advance from Playing without collecting: synthesize a Thinking
            // → Collected → advance_with cycle. Required when the user
            // "plays this move" without first asking the bots to think.
            let (a1, a2) = match actions {
                Some([a1, a2]) => (a1, a2),
                None => (EngineDirection::Stay, EngineDirection::Stay),
            };
            let thinking = p.start_turn().await.map_err(boxed)?;
            let collected = thinking.stop_and_collect().await.map_err(boxed)?;
            apply_advance_with(collected, a1, a2, reply).await
        },

        (state @ AnalysisState::Finished, _) => Ok(state),
    }
}

async fn start_turn(
    p: Match<Playing>,
    position: Option<AnalysisPosition>,
) -> Result<Match<Thinking>, Box<dyn std::error::Error + Send + Sync>> {
    match position {
        None => p.start_turn().await.map_err(boxed),
        Some(pos) => p
            .start_turn_with(turn_state_from_position(pos))
            .await
            .map_err(boxed),
    }
}

async fn advance_collected(
    c: Match<Collected>,
    actions: Option<[EngineDirection; 2]>,
    reply: oneshot::Sender<AnalysisResp>,
) -> Result<AnalysisState, Box<dyn std::error::Error + Send + Sync>> {
    match actions {
        Some([a1, a2]) => apply_advance_with(c, a1, a2, reply).await,
        None => {
            let (p1, p2) = outcome_directions(c.outcomes());
            match c.advance().await.map_err(boxed)? {
                StepResult::Continue(next) => {
                    let _ = reply.send(AnalysisResp::Advanced {
                        p1,
                        p2,
                        game_over: false,
                    });
                    Ok(AnalysisState::Playing(next))
                },
                StepResult::GameOver(finished) => {
                    let _ = reply.send(AnalysisResp::Advanced {
                        p1,
                        p2,
                        game_over: true,
                    });
                    let _ = finished.finalize().await;
                    Ok(AnalysisState::Finished)
                },
            }
        },
    }
}

async fn apply_advance_with(
    c: Match<Collected>,
    p1: EngineDirection,
    p2: EngineDirection,
    reply: oneshot::Sender<AnalysisResp>,
) -> Result<AnalysisState, Box<dyn std::error::Error + Send + Sync>> {
    match c.advance_with(p1, p2).await.map_err(boxed)? {
        StepResult::Continue(next) => {
            let _ = reply.send(AnalysisResp::Advanced {
                p1,
                p2,
                game_over: false,
            });
            Ok(AnalysisState::Playing(next))
        },
        StepResult::GameOver(finished) => {
            let _ = reply.send(AnalysisResp::Advanced {
                p1,
                p2,
                game_over: true,
            });
            let _ = finished.finalize().await;
            Ok(AnalysisState::Finished)
        },
    }
}

/// Map per-slot `ActionOutcome` to a `(Direction, Direction)` for the
/// frontend. `TimedOut` / `Disconnected` surface as `Stay` since the GUI
/// just wants "what did the bot say" and FaultPolicy hasn't run yet.
fn outcome_directions(
    outcomes: &[pyrat_host::match_host::ActionOutcome; 2],
) -> (EngineDirection, EngineDirection) {
    use pyrat_host::match_host::ActionOutcome;
    let dir = |o: &ActionOutcome| match o {
        ActionOutcome::Committed { direction, .. } => *direction,
        _ => EngineDirection::Stay,
    };
    (dir(&outcomes[0]), dir(&outcomes[1]))
}

/// Build a `pyrat_protocol::TurnState` for `Match::start_turn_with` from the
/// frontend's cursor-follows-analysis position.
fn turn_state_from_position(pos: AnalysisPosition) -> TurnState {
    TurnState {
        turn: pos.turn,
        player1_position: Coordinates::new(pos.player1.position.x, pos.player1.position.y),
        player2_position: Coordinates::new(pos.player2.position.x, pos.player2.position.y),
        player1_score: pos.player1.score,
        player2_score: pos.player2.score,
        player1_mud_turns: pos.player1.mud_turns,
        player2_mud_turns: pos.player2.mud_turns,
        cheese: pos
            .cheese
            .iter()
            .map(|c| Coordinates::new(c.x, c.y))
            .collect(),
        player1_last_move: specta_to_engine(pos.player1_last_move),
        player2_last_move: specta_to_engine(pos.player2_last_move),
    }
}

// ── Event forwarding ────────────────────────────────

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
                ..
            } => {
                let payload = TurnPlayedEvent {
                    match_id,
                    turn: state.turn,
                    state_hash: format!("{:016x}", state.state_hash()),
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
                    player1_action: engine_to_specta(p1_action),
                    player2_action: engine_to_specta(p2_action),
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
                let winner = match result.result {
                    GameResult::Player1 => MatchWinner::Player1,
                    GameResult::Player2 => MatchWinner::Player2,
                    GameResult::Draw => MatchWinner::Draw,
                    unknown => {
                        warn!(result = ?unknown, "unexpected GameResult variant");
                        MatchWinner::Draw
                    },
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
            MatchEvent::PreprocessingStarted => {
                let _ = PreprocessingStartedEvent { match_id }.emit(&app);
            },
            MatchEvent::SetupComplete => {
                let _ = SetupCompleteEvent { match_id }.emit(&app);
            },
            MatchEvent::BotInfo {
                sender,
                turn,
                state_hash,
                info,
            } => {
                let payload = BotInfoEvent {
                    match_id,
                    sender: player_side(sender),
                    subject: player_side(info.player),
                    turn,
                    state_hash: format!("{state_hash:016x}"),
                    multipv: info.multipv,
                    target: info.target.map(Coord::from),
                    depth: info.depth,
                    nodes: info.nodes,
                    score: info.score,
                    pv: info.pv.iter().map(|&d| engine_to_specta(d)).collect(),
                    message: info.message.clone(),
                };
                let _ = payload.emit(&app);
            },
            _ => {},
        }
    }
}
