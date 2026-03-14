use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use tokio::net::TcpListener;
use tokio::sync::mpsc;

use pyrat::game::builder::GameConfig;
use pyrat_host::game_loop::{
    accept_connections, build_owned_match_config, launch_bots, run_one_turn, run_setup, BotConfig,
    MatchEvent, MatchSetup, PlayerEntry, PlayingConfig, PlayingState, SetupTiming, TurnOutcome,
};
use pyrat_host::session::messages::SessionMsg;
use pyrat_host::session::SessionConfig;
use pyrat_host::stub::spawn_stub_bot;
use pyrat_host::wire::{Player, TimingMode};

use crate::manifest::BotManifest;

// ── Report types ─────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct CheckReport {
    pub bot_name: String,
    pub agent_id: String,
    pub passed: bool,
    pub phases: Vec<PhaseResult>,
}

#[derive(Debug, serde::Serialize)]
pub struct PhaseResult {
    pub name: &'static str,
    pub status: PhaseStatus,
    pub duration_ms: u64,
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
#[allow(dead_code)]
pub enum PhaseStatus {
    Pass { detail: String },
    Fail { detail: String },
    Skip { detail: String },
}

impl PhaseStatus {
    fn is_pass(&self) -> bool {
        matches!(self, Self::Pass { .. })
    }
}

impl PhaseResult {
    fn pass(name: &'static str, detail: impl Into<String>, elapsed: Duration) -> Self {
        Self {
            name,
            status: PhaseStatus::Pass {
                detail: detail.into(),
            },
            duration_ms: elapsed.as_millis() as u64,
        }
    }

    fn fail(name: &'static str, detail: impl Into<String>, elapsed: Duration) -> Self {
        Self {
            name,
            status: PhaseStatus::Fail {
                detail: detail.into(),
            },
            duration_ms: elapsed.as_millis() as u64,
        }
    }

    #[allow(dead_code)]
    fn skip(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: PhaseStatus::Skip {
                detail: detail.into(),
            },
            duration_ms: 0,
        }
    }
}

// ── Check flow ───────────────────────────────────────

pub async fn run_check(bot_dir: &Path) -> CheckReport {
    let bot_dir = match bot_dir.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return CheckReport {
                bot_name: "?".into(),
                agent_id: "?".into(),
                passed: false,
                phases: vec![PhaseResult::fail(
                    "manifest",
                    format!("bad path: {e}"),
                    Duration::ZERO,
                )],
            };
        },
    };

    let mut phases = Vec::new();

    // ── Phase 1: Parse manifest ──────────────────
    let t = Instant::now();
    let manifest = match BotManifest::load(&bot_dir) {
        Ok(m) => m,
        Err(e) => {
            phases.push(PhaseResult::fail("manifest", e.to_string(), t.elapsed()));
            return finish_report("?", "?", phases);
        },
    };
    phases.push(PhaseResult::pass(
        "manifest",
        "bot.toml parsed",
        t.elapsed(),
    ));

    let bot_name = manifest.settings.name.clone();
    let agent_id = manifest.settings.agent_id.clone();

    // ── Phase 2: Launch bot ──────────────────────
    let t = Instant::now();

    let mut game = match GameConfig::classic(7, 5, 3).create(Some(42)) {
        Ok(g) => g,
        Err(e) => {
            phases.push(PhaseResult::fail(
                "launch",
                format!("game creation failed: {e}"),
                t.elapsed(),
            ));
            return finish_report(&bot_name, &agent_id, phases);
        },
    };

    let match_config = build_owned_match_config(&game, TimingMode::Wait, 3000, 5000);

    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            phases.push(PhaseResult::fail(
                "launch",
                format!("failed to bind TCP: {e}"),
                t.elapsed(),
            ));
            return finish_report(&bot_name, &agent_id, phases);
        },
    };
    let port = listener.local_addr().unwrap().port();

    let bot_config = BotConfig {
        run_command: manifest.settings.run_command.clone(),
        working_dir: bot_dir.clone(),
        agent_id: agent_id.clone(),
    };

    let _bot_processes = match launch_bots(&[bot_config], port) {
        Ok(p) => p,
        Err(e) => {
            phases.push(PhaseResult::fail(
                "launch",
                format!("failed to spawn bot process: {e}. Check that run_command is valid."),
                t.elapsed(),
            ));
            return finish_report(&bot_name, &agent_id, phases);
        },
    };

    phases.push(PhaseResult::pass("launch", "process spawned", t.elapsed()));

    // ── Phase 3: Setup handshake ─────────────────
    let t = Instant::now();

    let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(64);
    let session_config = SessionConfig::default();
    tokio::spawn(accept_connections(
        listener,
        game_tx.clone(),
        session_config,
    ));

    // Spawn stub bot as Player 2.
    let stub_session_id = pyrat_host::session::SessionId(1000);
    let _stub_handle = spawn_stub_bot(stub_session_id, "__stub__".into(), "Stub".into(), game_tx);

    let setup = MatchSetup {
        players: vec![
            PlayerEntry {
                player: Player::Player1,
                agent_id: agent_id.clone(),
            },
            PlayerEntry {
                player: Player::Player2,
                agent_id: "__stub__".into(),
            },
        ],
        match_config,
        bot_options: HashMap::new(),
        timing: SetupTiming {
            startup_timeout: Duration::from_secs(30),
            preprocessing_timeout: Duration::from_secs(5),
        },
    };

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

    let setup_result = match run_setup(&setup, &mut game_rx, Some(&event_tx)).await {
        Ok(r) => r,
        Err(e) => {
            let detail = match &e {
                pyrat_host::game_loop::SetupError::StartupTimeout { .. } => {
                    format!("Bot did not connect and identify within 30s. Check that run_command starts and connects to $PYRAT_HOST_PORT. ({e})")
                },
                pyrat_host::game_loop::SetupError::BotDisconnected { .. } => {
                    format!(
                        "Bot disconnected during setup. Check for panics in startup code. ({e})"
                    )
                },
                pyrat_host::game_loop::SetupError::PreprocessingTimeout { .. } => {
                    format!("Bot didn't finish preprocessing within 5s. ({e})")
                },
                pyrat_host::game_loop::SetupError::AllDisconnected => {
                    format!("All sessions disconnected during setup. ({e})")
                },
            };
            phases.push(PhaseResult::fail("handshake", detail, t.elapsed()));
            return finish_report(&bot_name, &agent_id, phases);
        },
    };

    let handshake_duration = t.elapsed();

    // Scan events for BotIdentified to get bot's reported name.
    let mut identified_name = None;
    while let Ok(event) = event_rx.try_recv() {
        if let MatchEvent::BotIdentified {
            player: Player::Player1,
            ref name,
            ..
        } = event
        {
            identified_name = Some(name.clone());
        }
    }

    let detail = format!(
        "connect + identify + config + preprocess ({:.1}s)",
        handshake_duration.as_secs_f64()
    );
    phases.push(PhaseResult::pass("handshake", detail, handshake_duration));

    // ── Phase 4: Play 1 turn ─────────────────────
    let t = Instant::now();

    let playing_config = PlayingConfig {
        move_timeout: Duration::from_secs(3),
    };

    let mut playing_state = PlayingState::new(&setup_result.sessions);

    match run_one_turn(
        &mut playing_state,
        &mut game,
        &setup_result.sessions,
        &mut game_rx,
        &playing_config,
        Some(&event_tx),
    )
    .await
    {
        Ok(outcome) => {
            // Check if the bot timed out.
            let mut timed_out = false;
            let mut action_name = None;
            while let Ok(event) = event_rx.try_recv() {
                match &event {
                    MatchEvent::BotTimeout {
                        player: Player::Player1,
                        ..
                    } => {
                        timed_out = true;
                    },
                    MatchEvent::TurnPlayed { p1_action, .. } => {
                        action_name = Some(p1_action.variant_name().unwrap_or("?"));
                    },
                    _ => {},
                }
            }

            let detail = if timed_out {
                "turn 1 completed, but bot timed out (host used STAY as default)".to_string()
            } else {
                let action = action_name.unwrap_or("?");
                let outcome_str = match outcome {
                    TurnOutcome::Continue => "",
                    TurnOutcome::GameOver(_) => " (game over)",
                };
                format!("turn 1 completed, action: {action}{outcome_str}")
            };
            phases.push(PhaseResult::pass("play", detail, t.elapsed()));
        },
        Err(e) => {
            phases.push(PhaseResult::fail(
                "play",
                format!("Bot disconnected during first turn. ({e})"),
                t.elapsed(),
            ));
            return finish_report(
                identified_name.as_deref().unwrap_or(&bot_name),
                &agent_id,
                phases,
            );
        },
    }

    // ── Phase 5: Shutdown ────────────────────────
    let t = Instant::now();
    for s in &setup_result.sessions {
        let _ = s
            .cmd_tx
            .send(pyrat_host::session::messages::HostCommand::Shutdown)
            .await;
    }

    // Drain disconnect messages with 2s deadline.
    let session_count = setup_result.sessions.len();
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
                break;
            }
        }
    }
    // _bot_processes RAII guard kills the subprocess on drop.
    phases.push(PhaseResult::pass("shutdown", "clean", t.elapsed()));

    finish_report(
        identified_name.as_deref().unwrap_or(&bot_name),
        &agent_id,
        phases,
    )
}

fn finish_report(bot_name: &str, agent_id: &str, phases: Vec<PhaseResult>) -> CheckReport {
    let passed = phases.iter().all(|p| p.status.is_pass());
    CheckReport {
        bot_name: bot_name.into(),
        agent_id: agent_id.into(),
        passed,
        phases,
    }
}
