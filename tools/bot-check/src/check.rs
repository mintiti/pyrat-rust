use std::path::Path;
use std::time::{Duration, Instant};

use tokio::net::TcpListener;
use tokio::sync::mpsc;

use pyrat::{Direction, GameBuilder};
use pyrat_protocol::HashedTurnState;

use pyrat_host::launch::{launch_bots, BotConfig};
use pyrat_host::match_config::build_match_config;
use pyrat_host::match_host::{
    Match, MatchError, MatchEvent, PlayingConfig, SetupTiming, StepResult,
};
use pyrat_host::player::{
    accept_players, AcceptError, EmbeddedBot, EmbeddedCtx, EmbeddedPlayer, EventSink, Options,
    PlayerIdentity,
};
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
    Warn { detail: String },
    Fail { detail: String },
    Skip { detail: String },
}

impl PhaseStatus {
    fn is_pass(&self) -> bool {
        matches!(self, Self::Pass { .. } | Self::Warn { .. })
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

    fn warn(name: &'static str, detail: impl Into<String>, elapsed: Duration) -> Self {
        Self {
            name,
            status: PhaseStatus::Warn {
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

// ── In-tool opponent ─────────────────────────────────

/// Stay-only opponent. Combined with `with_max_turns(1)`, the match always
/// terminates on the turn limit, exercising the full setup → start → step →
/// finalize lifecycle in seconds.
struct CheckOpponent;

impl Options for CheckOpponent {}

impl EmbeddedBot for CheckOpponent {
    fn think(&mut self, _state: &HashedTurnState, _ctx: &EmbeddedCtx) -> Direction {
        Direction::Stay
    }
}

// ── Check flow ───────────────────────────────────────

const CANDIDATE_SLOT: Player = Player::Player1;
const OPPONENT_SLOT: Player = Player::Player2;
const OPPONENT_AGENT_ID: &str = "__check_opponent__";

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

    // Single-turn match: candidate connects, configures, preprocesses, plays
    // turn 1, sees GameOver from max-turn-limit, shuts down. Full lifecycle
    // exercised in seconds.
    let game = match GameBuilder::new(7, 5)
        .with_classic_maze()
        .with_corner_positions()
        .with_random_cheese(3, true)
        .with_max_turns(1)
        .build()
        .create(Some(42))
    {
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

    let match_config = build_match_config(&game, TimingMode::Wait, 3000, 5000);

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

    // ── Phase 3: Connect (TCP accept) ────────────
    let t = Instant::now();

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let event_sink = EventSink::new(event_tx.clone());

    let accept_result = accept_players(
        &listener,
        &[(CANDIDATE_SLOT, agent_id.clone())],
        event_sink.clone(),
        Duration::from_secs(30),
    )
    .await;

    let mut accepted = match accept_result {
        Ok(slots) => slots,
        Err(e) => {
            let detail = match &e {
                AcceptError::Timeout => format!(
                    "Bot did not connect and identify within 30s. \
                     Check that run_command starts and connects to $PYRAT_HOST_PORT. ({e})"
                ),
                _ => format!("accept failed: {e}"),
            };
            phases.push(PhaseResult::fail("connect", detail, t.elapsed()));
            return finish_report(&bot_name, &agent_id, phases);
        },
    };

    let candidate = match accepted[0].take() {
        Some(p) => p,
        None => {
            phases.push(PhaseResult::fail(
                "connect",
                "candidate slot did not receive a connection",
                t.elapsed(),
            ));
            return finish_report(&bot_name, &agent_id, phases);
        },
    };

    phases.push(PhaseResult::pass(
        "connect",
        "TCP accepted, Identify received",
        t.elapsed(),
    ));

    // ── Build the embedded opponent (post-Welcome) ──
    let opponent_identity = PlayerIdentity {
        name: "Check Opponent".into(),
        author: "pyrat-check".into(),
        agent_id: OPPONENT_AGENT_ID.into(),
        slot: OPPONENT_SLOT,
    };
    let opponent =
        match EmbeddedPlayer::accept(CheckOpponent, opponent_identity, event_sink.clone()).await {
            Ok(p) => p,
            Err(e) => {
                phases.push(PhaseResult::fail(
                    "connect",
                    format!("failed to build embedded opponent: {e}"),
                    Duration::ZERO,
                ));
                return finish_report(&bot_name, &agent_id, phases);
            },
        };

    // ── Phase 4: Setup handshake (Configure → Ready → Preprocess) ──
    let t = Instant::now();

    let m = Match::new(
        game,
        [Box::new(candidate), Box::new(opponent)],
        match_config,
        [Vec::new(), Vec::new()],
        SetupTiming {
            configure_timeout: Duration::from_secs(5),
            preprocessing_timeout: Duration::from_secs(5),
        },
        PlayingConfig {
            move_timeout: Duration::from_secs(3),
            ..PlayingConfig::default()
        },
        Some(event_tx.clone()),
    );

    let m = match m.setup().await {
        Ok(m) => m,
        Err(e) => {
            let detail = format_setup_error(&e);
            phases.push(PhaseResult::fail("handshake", detail, t.elapsed()));
            return finish_report(&bot_name, &agent_id, phases);
        },
    };

    phases.push(PhaseResult::pass(
        "handshake",
        "configure + ready + preprocess",
        t.elapsed(),
    ));

    // ── Phase 5: Play one turn (turn loop, max_turns=1 ends it) ──
    let t = Instant::now();
    let m = m.start();

    let finished = match m.step().await {
        Ok(StepResult::Continue(_)) => {
            // max_turns=1 means the engine stops after turn 1; we should
            // never see Continue. If we do, that's a bug in the test setup.
            phases.push(PhaseResult::fail(
                "play",
                "expected GameOver after turn 1 (max_turns=1)",
                t.elapsed(),
            ));
            return finish_report(&bot_name, &agent_id, phases);
        },
        Ok(StepResult::GameOver(finished)) => finished,
        Err(e) => {
            let detail = format_play_error(&e);
            phases.push(PhaseResult::fail("play", detail, t.elapsed()));
            return finish_report(&bot_name, &agent_id, phases);
        },
    };

    // Inspect collected events for candidate-specific signals.
    let mut candidate_timed_out = false;
    let mut candidate_action: Option<String> = None;
    let mut identified_name: Option<String> = None;
    while let Ok(event) = event_rx.try_recv() {
        match event {
            MatchEvent::BotIdentified {
                player, ref name, ..
            } if player == CANDIDATE_SLOT => {
                identified_name = Some(name.clone());
            },
            MatchEvent::BotTimeout { player, .. } if player == CANDIDATE_SLOT => {
                candidate_timed_out = true;
            },
            MatchEvent::TurnPlayed { p1_action, .. } => {
                candidate_action = Some(format!("{p1_action:?}"));
            },
            _ => {},
        }
    }

    if candidate_timed_out {
        phases.push(PhaseResult::warn(
            "play",
            "turn 1 completed, but bot timed out (host fell back to provisional / Stay)",
            t.elapsed(),
        ));
    } else {
        let action = candidate_action.as_deref().unwrap_or("?");
        phases.push(PhaseResult::pass(
            "play",
            format!("turn 1 completed, action: {action} (game over by max-turn limit)"),
            t.elapsed(),
        ));
    }

    // ── Phase 6: Shutdown ────────────────────────
    let t = Instant::now();
    let _result = finished.finalize().await;
    phases.push(PhaseResult::pass("shutdown", "clean", t.elapsed()));
    // _bot_processes RAII guard kills the candidate subprocess on drop.

    finish_report(
        identified_name.as_deref().unwrap_or(&bot_name),
        &agent_id,
        phases,
    )
}

fn format_setup_error(e: &MatchError) -> String {
    match e {
        MatchError::SetupTimeout(_) => format!(
            "Setup did not complete within the timeout (5s configure / 5s preprocess). \
             Check for hangs in startup or preprocess. ({e})"
        ),
        MatchError::ReadyHashMismatch { .. } => format!(
            "Bot reported a state hash that doesn't match the host's. \
             SDK and engine state out of sync. ({e})"
        ),
        MatchError::BotDisconnected(_) => {
            format!("Bot disconnected during setup. Check for panics in startup code. ({e})")
        },
        _ => format!("setup failed: {e}"),
    }
}

fn format_play_error(e: &MatchError) -> String {
    match e {
        MatchError::BotDisconnected(_) => {
            format!("Bot disconnected during play. ({e})")
        },
        _ => format!("play failed: {e}"),
    }
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
