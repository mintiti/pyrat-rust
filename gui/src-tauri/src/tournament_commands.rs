//! Tauri commands for the tournament module.
//!
//! `start_tournament` creates the row (returning its id) and spawns the
//! runner on a background task. The store is the same SQLite the CLI uses, at
//! the app-data path (`tournament_paths`). Read commands open a fresh
//! connection each call — WAL allows concurrent readers, and they're
//! infrequent.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use pyrat::game::builder::{
    CheeseStrategy, GameBuilder, GameConfig, MazeParams, MazeStrategy, PlayerStrategy,
};
use pyrat_eval::{
    EvalSession, MatchupKey, ResolvedPlayer, SeatPolicy, TournamentMethodology, TournamentParams,
    TournamentSpec, TournamentState, TournamentTimingMode,
};
use pyrat_eval_store::{AttemptOutcome, EvalStore, SeatOrientation, TournamentId};
use pyrat_host::probe::{preflight_bot, PreflightConfig};
use pyrat_orchestrator::{PlayerSpec, ReplayEvent, ReplayFile};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::commands::{MazeState, MudEntry, PlayerState, WallEntry};
use crate::state::{AppState, TournamentPhase};
use crate::tournament_config;
use crate::tournament_events::{FailureKind, StandingRow, TimeoutPhase, TournamentPreparingEvent};
use crate::tournament_runner::{
    build_standings, ordered_player_ids, run_tournament, total_games, RunnerFormat, TournamentRun,
};

// ---------------------------------------------------------------------------
// Command I/O types (specta-exported)
// ---------------------------------------------------------------------------

/// One bot selected on the launch screen. `agent_id` is the stable bot.toml id
/// and doubles as the tournament-scoped player id.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct BotPick {
    pub agent_id: String,
    pub run_command: String,
    pub working_dir: String,
}

/// Tournament seeds round-trip through the frontend as a JS `number`, exact
/// only up to 2^53 - 1. Generate / accept seeds in that range so a reproduced
/// tournament uses the same seed the user saw. (The CLI keeps the full `u64`;
/// it never crosses a JS boundary.)
const MAX_JS_SAFE_SEED: u64 = (1 << 53) - 1;

/// Player start strategy for the game-instance factory. Mirrors the engine's
/// `PlayerStrategy` (corners | random); fixed positions are parked.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum PlayerStart {
    Corners,
    Random,
}

/// The game-instance distribution configured on the launch screen: board,
/// maze, start strategy, cheese. This is the *distribution*; the tournament
/// seed that selects which instances get drawn lives on `LaunchParams`.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct GameFactoryConfig {
    pub width: u32,
    pub height: u32,
    pub max_turns: u32,
    pub wall_density: f64,
    pub mud_density: f64,
    pub mud_range: u32,
    pub connected: bool,
    pub symmetric: bool,
    pub player_start: PlayerStart,
    pub cheese_count: u32,
    pub cheese_symmetric: bool,
}

impl GameFactoryConfig {
    /// Build the engine `GameConfig`, validating engine bounds (the builder
    /// asserts on out-of-range dims / zero max_turns) and the seed-independent
    /// random-start invariants. A single `create(seed)` smoke test (in
    /// `build_and_spawn`) catches per-seed cheese-capacity failures; random
    /// starts draw many seeds, so symmetric-cheese soundness is enforced here
    /// instead — no seed can fail mid-tournament.
    fn to_game_config(&self) -> Result<GameConfig, String> {
        if !(2..=255).contains(&self.width) {
            return Err(format!("width must be 2..=255, got {}", self.width));
        }
        if !(2..=255).contains(&self.height) {
            return Err(format!("height must be 2..=255, got {}", self.height));
        }
        if !(1..=u32::from(u16::MAX)).contains(&self.max_turns) {
            return Err(format!(
                "max_turns must be 1..=65535, got {}",
                self.max_turns
            ));
        }
        if self.mud_range > u32::from(u8::MAX) {
            return Err(format!("mud_range must be <= 255, got {}", self.mud_range));
        }
        if self.mud_density > 0.0 && self.mud_range < 2 {
            return Err("mud_range must be >= 2 when mud is enabled".into());
        }
        if self.cheese_count < 1 {
            return Err("cheese_count must be >= 1".into());
        }
        if self.cheese_count > u32::from(u16::MAX) {
            return Err(format!(
                "cheese_count must be <= 65535, got {}",
                self.cheese_count
            ));
        }

        let area = self.width.saturating_mul(self.height);
        if matches!(self.player_start, PlayerStart::Random) && self.cheese_symmetric {
            if self.cheese_count % 2 == 1 {
                return Err(
                    "random starts with symmetric cheese need an even cheese count: \
                     the unpaired (center) piece can't be placed when a random start \
                     occupies the center"
                        .into(),
                );
            }
            // A random pair removes up to two mirror-pairs of cells (each player
            // + its mirror); an odd×odd board also reserves the self-mirror
            // center, which can't hold a paired piece.
            let odd_board = self.width % 2 == 1 && self.height % 2 == 1;
            let cap = area.saturating_sub(if odd_board { 5 } else { 4 });
            if self.cheese_count > cap {
                return Err(format!(
                    "random + symmetric cheese over capacity: max {cap} on this board, got {}",
                    self.cheese_count
                ));
            }
        }

        let maze = MazeParams {
            wall_density: self.wall_density as f32,
            connected: self.connected,
            symmetric: self.symmetric,
            mud_density: self.mud_density as f32,
            mud_range: self.mud_range as u8,
        };
        let builder = GameBuilder::new(self.width as u8, self.height as u8)
            .with_max_turns(self.max_turns as u16)
            .with_random_maze(maze);
        let builder = match self.player_start {
            PlayerStart::Corners => builder.with_corner_positions(),
            PlayerStart::Random => builder.with_random_positions(),
        };
        Ok(builder
            .with_random_cheese(self.cheese_count as u16, self.cheese_symmetric)
            .build())
    }
}

/// Build a factory config from an engine `GameConfig` (e.g. a preset) — the
/// inverse of [`GameFactoryConfig::to_game_config`]. Used for launch defaults
/// so they mirror the actual preset rather than a hand-copied constant (the
/// stale-mirror class of bug).
fn factory_from_game_config(cfg: &GameConfig) -> Result<GameFactoryConfig, String> {
    let MazeStrategy::Random(p) = cfg.maze() else {
        return Err("default config uses a fixed maze (not representable as a factory)".into());
    };
    let CheeseStrategy::Random { count, symmetric } = cfg.cheese() else {
        return Err("default config uses fixed cheese".into());
    };
    let player_start = match cfg.players() {
        PlayerStrategy::Corners => PlayerStart::Corners,
        PlayerStrategy::Random => PlayerStart::Random,
        PlayerStrategy::Fixed(..) => return Err("default config uses fixed positions".into()),
    };
    Ok(GameFactoryConfig {
        width: u32::from(cfg.width()),
        height: u32::from(cfg.height()),
        max_turns: u32::from(cfg.max_turns()),
        wall_density: f64::from(p.wall_density),
        mud_density: f64::from(p.mud_density),
        mud_range: u32::from(p.mud_range),
        connected: p.connected,
        symmetric: p.symmetric,
        player_start,
        cheese_count: u32::from(*count),
        cheese_symmetric: *symmetric,
    })
}

/// Launch parameters. The factory + methodology knobs are configured on the
/// launch screen; the frontend pre-fills them from `get_tournament_launch_defaults`.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct LaunchParams {
    pub bots: Vec<BotPick>,
    /// The starred bot to measure → gauntlet. `None` → round-robin.
    pub target: Option<String>,
    pub name: Option<String>,
    /// The game-instance distribution (board / maze / starts / cheese).
    pub factory: GameFactoryConfig,
    /// Mazes per matchup; the paired schedule runs 2× this many games.
    pub mazes_per_matchup: u32,
    pub move_timeout_ms: u32,
    pub preprocessing_timeout_ms: u32,
    pub max_parallel: u32,
    /// Tournament seed (selects which instances are drawn). `None` → random,
    /// capped to the JS-safe range so it round-trips for reproducibility.
    pub tournament_seed: Option<u64>,
}

/// Launch-form defaults: the ladder recipe as a factory config plus the
/// methodology knobs. The frontend pre-fills from this, so the Rust constants
/// stay the single source of truth (no stale TS mirror to drift).
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct LaunchDefaults {
    pub factory: GameFactoryConfig,
    pub mazes_per_matchup: u32,
    pub move_timeout_ms: u32,
    pub preprocessing_timeout_ms: u32,
    pub max_parallel: u32,
}

/// Row in the "in this store" panel.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct TournamentSummary {
    pub id: i64,
    pub name: Option<String>,
    pub format: String,
    pub created_at: String,
    /// The app-owned runner is currently executing this tournament. Separate
    /// from `finished`: navigation never owns runner lifetime.
    pub running: bool,
    /// All expected (pair, repetition) slots are done (success or
    /// failure-exhausted) — `slot_done` semantics, not raw count-vs-target.
    pub finished: bool,
    pub done: u32,
    pub total: u32,
}

/// Standings for a finished/partial tournament, reopened from the store.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct StandingsSnapshot {
    pub tournament_id: i64,
    pub name: Option<String>,
    pub format: String,
    pub anchor_id: String,
    pub finished: bool,
    pub done: u32,
    pub total: u32,
    pub standings: Vec<StandingRow>,
}

/// One durable successful attempt. Canonical player ids/scores stay stable
/// across the two seat-swapped legs; `rat_id` makes the actual seat visible.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct StoredFinishedGame {
    pub attempt_id: i64,
    /// Absent on rows written before migration 6. Those results remain
    /// inspectable, but have no direct replay link.
    pub match_id: Option<u64>,
    pub player1_id: String,
    pub player2_id: String,
    pub repetition_index: u32,
    pub rat_id: String,
    pub player1_score: f64,
    pub player2_score: f64,
}

/// One durable failed attempt, retained in historical tournament inspection.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct StoredMatchFailure {
    pub attempt_id: i64,
    pub match_id: Option<u64>,
    pub player1_id: String,
    pub player2_id: String,
    pub repetition_index: u32,
    pub rat_id: String,
    pub failing_player_id: Option<String>,
    pub kind: FailureKind,
    pub timeout_phase: Option<TimeoutPhase>,
    pub reason: String,
}

/// Wire-safe projection of the store-native timing mode.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum StoredTimingMode {
    Wait,
    Clock,
}

/// Durable execution conditions for a tournament. Kept as one optional
/// object: a missing object means the pre-migration row did not record any of
/// these values, rather than inheriting today's launch defaults.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct StoredTournamentMethodology {
    pub timing_mode: StoredTimingMode,
    pub move_timeout_ms: u32,
    pub preprocessing_timeout_ms: u32,
    pub startup_timeout_ms: u32,
    pub configure_timeout_ms: u32,
    pub network_grace_ms: u32,
    pub max_parallel: u32,
}

impl From<TournamentMethodology> for StoredTournamentMethodology {
    fn from(value: TournamentMethodology) -> Self {
        Self {
            timing_mode: match value.timing_mode {
                TournamentTimingMode::Wait => StoredTimingMode::Wait,
                TournamentTimingMode::Clock => StoredTimingMode::Clock,
            },
            move_timeout_ms: value.move_timeout_ms,
            preprocessing_timeout_ms: value.preprocessing_timeout_ms,
            startup_timeout_ms: value.startup_timeout_ms,
            configure_timeout_ms: value.configure_timeout_ms,
            network_grace_ms: value.network_grace_ms,
            max_parallel: value.max_parallel,
        }
    }
}

/// Complete read model for one inspectable tournament. The same command
/// hydrates historical rows and reconciles an active event-fed view from
/// durable truth when lifecycle broadcasts lag.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct TournamentSnapshot {
    pub tournament_id: i64,
    pub name: Option<String>,
    pub format: String,
    pub target: Option<String>,
    pub anchor_id: String,
    pub running: bool,
    pub finished: bool,
    pub paired: bool,
    /// Absent only when an older tournament row did not record its execution
    /// conditions. Optional in TypeScript so existing fixture consumers remain
    /// compatible; current rows always include it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub methodology: Option<StoredTournamentMethodology>,
    pub created_at: String,
    pub last_finished_at: Option<String>,
    pub plan_summary: String,
    pub players: Vec<String>,
    pub games_per_matchup: u32,
    pub done: u32,
    pub total: u32,
    pub success: u32,
    pub failure: u32,
    pub standings: Vec<StandingRow>,
    pub games: Vec<StoredFinishedGame>,
    pub failures: Vec<StoredMatchFailure>,
}

/// Final-position board + verdict for one finished game, or a reason it's
/// unavailable (failed match, or replay file missing).
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GameReplayState {
    Available {
        final_state: MazeState,
        /// Winning player id; `None` for a draw.
        winner: Option<String>,
        player1_id: String,
        player2_id: String,
        player1_score: f32,
        player2_score: f32,
        turns: u16,
    },
    Missing {
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Whether a new tournament may claim the slot.
///
/// `Idle` is free. A `Running` slot whose runner task has already **finished**
/// is reclaimable: the runner doesn't reset the phase on natural finish (it
/// never touches `AppState`), so the slot would otherwise stay `Running`
/// forever and refuse every future launch — the "can't launch after a
/// tournament finishes" bug. A still-running `Running` or an in-flight
/// `Starting` is refused, with distinct messages.
fn reservation_check(phase: &TournamentPhase) -> Result<(), String> {
    match phase {
        TournamentPhase::Idle => Ok(()),
        TournamentPhase::Running { handle, .. } if handle.is_finished() => Ok(()),
        TournamentPhase::Starting { .. } => Err("a tournament is already starting".into()),
        TournamentPhase::Running { .. } => Err("a tournament is already running".into()),
    }
}

async fn current_running_tournament(state: &AppState) -> Option<i64> {
    let phase = state.tournament_phase.lock().await;
    match &*phase {
        TournamentPhase::Running {
            tournament_id,
            handle,
            ..
        } if !handle.is_finished() => Some(*tournament_id),
        _ => None,
    }
}

/// Create a tournament and start running it in the background. Returns the
/// new tournament id only after the runner acknowledges that its session is
/// live. Rejects if one is already running.
#[tauri::command]
#[specta::specta]
pub async fn start_tournament(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    params: LaunchParams,
) -> Result<i64, String> {
    if params.bots.len() < 2 {
        return Err("a tournament needs at least 2 bots".into());
    }
    if let Some(t) = &params.target {
        if !params.bots.iter().any(|b| &b.agent_id == t) {
            return Err(format!("target `{t}` is not in the selected bots"));
        }
    }

    // Reserve the slot up front, under the lock, *before* the async
    // create/open-store/spawn work. Without this, two concurrent starts both
    // pass an `Idle` check while the lock is released across the awaits and
    // the second orphans the first's runner. Every path below must release
    // the reservation (rollback to Idle) on error or honor a stop that lands
    // mid-launch.
    // The cancel token is born here, with the reservation, and stored in
    // `Starting` so a `stop_tournament` during the (possibly slow) warmup can
    // fire it immediately. The same token is reused as the runner's token on
    // promotion, so cancellation spans the whole start lifetime.
    let cancel = {
        let mut phase = state.tournament_phase.lock().await;
        reservation_check(&phase)?;
        let cancel = CancellationToken::new();
        *phase = TournamentPhase::Starting {
            cancel: cancel.clone(),
        };
        cancel
    };

    match build_and_spawn(&app, params, cancel.clone()).await {
        Ok((tournament_id, handle, startup)) => match startup.await {
            Ok(Ok(())) => {
                let mut phase = state.tournament_phase.lock().await;
                if matches!(*phase, TournamentPhase::Starting { .. }) {
                    *phase = TournamentPhase::Running {
                        tournament_id,
                        cancel,
                        handle,
                    };
                    Ok(tournament_id)
                } else {
                    // A `stop_tournament` landed during the launch window (it reset
                    // the slot to Idle and already fired the token). Honor it:
                    // cancel (idempotent) and drain the runner we just spawned,
                    // rather than promoting a tournament the user asked to stop.
                    cancel.cancel();
                    let _ = handle.await;
                    Err("tournament start was cancelled".into())
                }
            },
            Ok(Err(error)) => {
                let _ = handle.await;
                let mut phase = state.tournament_phase.lock().await;
                if matches!(*phase, TournamentPhase::Starting { .. }) {
                    *phase = TournamentPhase::Idle;
                }
                Err(error)
            },
            Err(_) => {
                let _ = handle.await;
                let mut phase = state.tournament_phase.lock().await;
                if matches!(*phase, TournamentPhase::Starting { .. }) {
                    *phase = TournamentPhase::Idle;
                }
                Err("tournament runner exited before startup completed".into())
            },
        },
        Err(e) => {
            // Roll back the reservation so a failed launch can't wedge the slot.
            // (If a stop already reset it to Idle, leave it.)
            let mut phase = state.tournament_phase.lock().await;
            if matches!(*phase, TournamentPhase::Starting { .. }) {
                *phase = TournamentPhase::Idle;
            }
            Err(e)
        },
    }
}

/// Launch-form defaults the frontend pre-fills from: the ladder recipe as a
/// factory config plus the methodology knobs. Derived from the Rust constants
/// (and the actual preset), so there's no TS mirror to drift.
#[tauri::command]
#[specta::specta]
pub async fn get_tournament_launch_defaults() -> Result<LaunchDefaults, String> {
    let factory = factory_from_game_config(&tournament_config::game_config()?)?;
    Ok(LaunchDefaults {
        factory,
        mazes_per_matchup: tournament_config::MAZES_PER_MATCHUP,
        move_timeout_ms: tournament_config::MOVE_TIMEOUT_MS,
        preprocessing_timeout_ms: tournament_config::PREPROCESSING_TIMEOUT_MS,
        max_parallel: tournament_config::MAX_PARALLEL,
    })
}

/// Create the tournament row and spawn its runner. Returns the new id plus the
/// runner handle and startup acknowledgement; the caller installs the live
/// runner (with the `cancel` it owns) into `TournamentPhase::Running`. Phase
/// management lives entirely in
/// `start_tournament`; this is the fallible build body it wraps so every error
/// path releases the `Starting` slot. `cancel` is the start-lifetime token: it
/// gates warmup and is reused as the runner's token.
async fn build_and_spawn(
    app: &tauri::AppHandle,
    params: LaunchParams,
    cancel: CancellationToken,
) -> Result<
    (
        i64,
        tokio::task::JoinHandle<()>,
        tokio::sync::oneshot::Receiver<Result<(), String>>,
    ),
    String,
> {
    let store = open_store(app)?;
    let players = resolve_players(&params.bots);

    let (format, format_str) = match &params.target {
        Some(target) => {
            let opponents = params
                .bots
                .iter()
                .map(|b| b.agent_id.clone())
                .filter(|id| id != target)
                .collect();
            (
                RunnerFormat::Gauntlet {
                    challenger: target.clone(),
                    opponents,
                },
                "gauntlet".to_string(),
            )
        },
        None => (RunnerFormat::RoundRobin, "round_robin".to_string()),
    };

    let player_ids = ordered_player_ids(&format, &players);
    // Register players in the SAME canonical (slot) order the planner expects —
    // for a gauntlet that's challenger-first. Otherwise the stored tournament's
    // player order diverges from the planner's and start-up validation rejects
    // it. Mirrors the CLI's `canonical_players`.
    let canonical_players: Vec<ResolvedPlayer> = player_ids
        .iter()
        .filter_map(|id| players.iter().find(|p| &p.id == id).cloned())
        .collect();
    let anchor_id = tournament_config::derive_anchor(&player_ids, params.target.as_deref())
        .ok_or("could not pick an Elo anchor (need a non-target player)")?;

    // Resolve & validate the measurement conditions once, here — the runner
    // consumes these rather than reaching back into pinned constants.
    let game_config = params.factory.to_game_config()?;
    let target_games_per_matchup = params.mazes_per_matchup.saturating_mul(2);
    if target_games_per_matchup == 0 {
        return Err("mazes_per_matchup must be >= 1".into());
    }
    let tournament_seed = match params.tournament_seed {
        Some(s) if s > MAX_JS_SAFE_SEED => {
            return Err(format!(
                "tournament_seed must be <= {MAX_JS_SAFE_SEED} (JS-safe range)"
            ));
        },
        Some(s) => s,
        None => fastrand::u64(0..=MAX_JS_SAFE_SEED),
    };
    // Smoke-test at the tournament seed: an invalid config fails here (clear
    // message, no dangling row) rather than inside the runner. For random
    // starts this validates only one draw; the seed-independent invariants in
    // `to_game_config` cover the rest.
    let representative_game = game_config
        .create(Some(tournament_seed))
        .map_err(|e| format!("invalid game config: {e}"))?;

    let seat_policy = tournament_config::SEAT_POLICY;
    let timing = tournament_config::per_match_timing(
        params.move_timeout_ms,
        params.preprocessing_timeout_ms,
    );
    let orchestrator_config = tournament_config::orchestrator_config(
        params.move_timeout_ms,
        params.preprocessing_timeout_ms,
        params.max_parallel,
    );

    let total = total_games(&format, canonical_players.len(), target_games_per_matchup);
    let plan_summary = plan_summary(
        &params.target,
        canonical_players.len(),
        &params.factory,
        target_games_per_matchup,
        params.move_timeout_ms,
    );

    let spec = TournamentSpec {
        name: params.name.clone(),
        format: format_str.clone(),
        target_games_per_matchup: Some(target_games_per_matchup),
        params_json: TournamentParams {
            max_failures_per_pair: tournament_config::MAX_FAILURES_PER_PAIR,
            seat_policy,
        }
        .to_json(),
        methodology: Some(TournamentMethodology {
            timing_mode: TournamentTimingMode::Wait,
            move_timeout_ms: timing.move_timeout_ms,
            preprocessing_timeout_ms: timing.preprocessing_timeout_ms,
            startup_timeout_ms: tournament_config::STARTUP_TIMEOUT_MS,
            configure_timeout_ms: tournament_config::CONFIGURE_TIMEOUT_MS,
            network_grace_ms: tournament_config::NETWORK_GRACE_MS,
            max_parallel: params.max_parallel.max(1),
        }),
        game_config: game_config.clone(),
        tournament_seed,
    };

    // Warm up the bots *before* creating the tournament row: pay the one-time
    // cold-build / dep-resolve cost up front (so the first matches don't time
    // out cold), drain build chatter before matches, and catch a dead bot here
    // (clear error, no dangling row) rather than mid-run. Aborts cleanly on a
    // stop landing during warmup.
    let preflight = PreflightConfig::new(
        &representative_game,
        timing.mode,
        timing.move_timeout_ms,
        timing.preprocessing_timeout_ms,
        orchestrator_config.handshake_timeout,
        orchestrator_config.setup_timing.configure_timeout,
    );
    warmup_bots(app, &params.bots, &cancel, preflight).await?;

    let created =
        EvalSession::create_tournament(store.clone(), spec.clone(), canonical_players.clone())
            .await
            .map_err(|e| format!("create tournament: {e}"))?;
    let tournament_id = created.tournament_id;

    let replay_dir = crate::tournament_paths::tournament_replay_dir(app, tournament_id.0)?;
    let run = TournamentRun {
        tournament_id,
        game_config_id: created.game_config_id,
        tournament_seed,
        name: params.name,
        format_str,
        players: canonical_players,
        format,
        target: params.target,
        anchor_id,
        plan_summary,
        total_games: total,
        player_ids,
        replay_dir,
        game_config,
        target_games_per_matchup,
        seat_policy,
        timing,
        orchestrator_config,
    };

    let app_for_task = app.clone();
    let (startup_tx, startup_rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(async move {
        if let Err(e) = run_tournament(app_for_task, store, run, cancel, startup_tx).await {
            warn!(error = %e, "tournament runner exited with error");
        }
    });

    Ok((tournament_id.0, handle, startup_rx))
}

/// Smoke-build each distinct bot once before the tournament starts.
///
/// Pays the one-time cold `cargo build` / `uv` dep-resolve cost up front so the
/// first matches don't time out cold, drains build chatter before matches
/// begin, and catches a bot that can't start *here* (clear named error, no
/// tournament row) instead of as a mid-run failure cascade.
///
/// Sequential by design: distinct bots may share a Cargo target dir (AlphaRat's
/// crates do), so warming them concurrently would contend on the build lock,
/// and parallel release builds thrash a dev machine. The cost is paid once.
/// `cancel` makes a stop during a long warmup responsive — the in-flight probe
/// future is dropped, which kills its bot subprocess via `BotProcesses` drop.
/// Distinct bots by `agent_id`, first-seen order — round-robin reuses a bot
/// across many matchups but it has one target dir to warm.
fn distinct_by_agent(bots: &[BotPick]) -> Vec<&BotPick> {
    let mut seen = HashSet::new();
    bots.iter()
        .filter(|b| seen.insert(b.agent_id.as_str()))
        .collect()
}

async fn warmup_bots(
    app: &tauri::AppHandle,
    bots: &[BotPick],
    cancel: &CancellationToken,
    preflight: PreflightConfig,
) -> Result<(), String> {
    let distinct = distinct_by_agent(bots);
    let total = distinct.len() as u32;
    for (i, bot) in distinct.iter().enumerate() {
        let _ = TournamentPreparingEvent {
            done: i as u32,
            total,
            current: Some(bot.agent_id.clone()),
        }
        .emit(app);
        let probe = preflight_bot(
            bot.run_command.clone(),
            bot.working_dir.clone(),
            bot.agent_id.clone(),
            preflight.clone(),
        );
        tokio::select! {
            biased;
            () = cancel.cancelled() => return Err("tournament start was cancelled".into()),
            result = probe => {
                result.map_err(|e| format!("bot `{}` failed compatibility check: {e}", bot.agent_id))?;
            }
        }
    }
    let _ = TournamentPreparingEvent {
        done: total,
        total,
        current: None,
    }
    .emit(app);
    Ok(())
}

/// Request the running tournament to stop and wait for it to drain. The runner
/// shuts the session down gracefully and emits `TournamentAbortedEvent`. No-op
/// if nothing is running.
#[tauri::command]
#[specta::specta]
pub async fn stop_tournament(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let prev = {
        let mut phase = state.tournament_phase.lock().await;
        std::mem::replace(&mut *phase, TournamentPhase::Idle)
    };
    match prev {
        TournamentPhase::Running { cancel, handle, .. } => {
            cancel.cancel();
            let _ = handle.await;
        },
        // A start is mid-flight (likely warming up bots). Fire its token so the
        // in-flight `build_and_spawn` aborts promptly; resetting the slot to
        // Idle (above) makes `start_tournament` return the cancelled error
        // instead of promoting to Running.
        TournamentPhase::Starting { cancel } => cancel.cancel(),
        TournamentPhase::Idle => {},
    }
    Ok(())
}

/// The currently-running tournament id, if any. The frontend reads this on
/// load / tab switch to restore the live chip after a navigation.
#[tauri::command]
#[specta::specta]
pub async fn tournament_status(state: tauri::State<'_, AppState>) -> Result<Option<i64>, String> {
    // Only a *live* runner counts. Starting has no row/id yet, and a finished
    // handle may still occupy the phase slot until the next reservation.
    Ok(current_running_tournament(&state).await)
}

/// List tournaments in the store, newest first, with finished-inference.
#[tauri::command]
#[specta::specta]
pub async fn list_tournaments(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<TournamentSummary>, String> {
    let running_id = current_running_tournament(&state).await;
    let store = open_store(&app)?;
    let g = store.lock();
    let records = g
        .list_tournaments()
        .map_err(|e| format!("list tournaments: {e}"))?;

    let mut out = Vec::with_capacity(records.len());
    for rec in records {
        let players: Vec<String> = g
            .get_tournament_players(rec.id)
            .map_err(|e| format!("tournament players: {e}"))?
            .into_iter()
            .map(|p| p.player_id)
            .collect();
        let attempts = g
            .get_attempts(rec.id, None)
            .map_err(|e| format!("attempts: {e}"))?;
        let mut st = TournamentState::empty(rec.id);
        for a in &attempts {
            st.fold_attempt(a);
        }
        let target = rec.target_games_per_matchup.unwrap_or(0);
        let max_failures = TournamentParams::from_json(&rec.params_json)
            .map(|p| p.max_failures_per_pair)
            .unwrap_or(0);
        let total = matchup_count(&rec.format, players.len()) as u32 * target;
        let done = success_count(&st);
        let finished = tournament_finished(
            &st,
            &rec.format,
            &players,
            &rec.game_config_id,
            target,
            max_failures,
        );
        out.push(TournamentSummary {
            id: rec.id.0,
            name: rec.name,
            format: rec.format,
            created_at: rec.created_at,
            running: running_id == Some(rec.id.0),
            finished,
            done,
            total,
        });
    }
    out.reverse(); // list_tournaments returns ascending id; show newest first.
    Ok(out)
}

/// Standings for one tournament, reopened from the store (read-only). Used for
/// finished tournaments and interrupted ones (standings-so-far).
#[tauri::command]
#[specta::specta]
pub async fn get_tournament_standings(
    app: tauri::AppHandle,
    tournament_id: i64,
) -> Result<StandingsSnapshot, String> {
    let store = open_store(&app)?;
    let g = store.lock();
    let tid = TournamentId(tournament_id);
    let rec = g
        .get_tournament(tid)
        .map_err(|e| format!("get tournament: {e}"))?
        .ok_or_else(|| format!("tournament {tournament_id} not found"))?;
    let players: Vec<String> = g
        .get_tournament_players(tid)
        .map_err(|e| format!("tournament players: {e}"))?
        .into_iter()
        .map(|p| p.player_id)
        .collect();
    let attempts = g
        .get_attempts(tid, None)
        .map_err(|e| format!("attempts: {e}"))?;
    drop(g);

    let mut st = TournamentState::empty(tid);
    for a in &attempts {
        st.fold_attempt(a);
    }

    // Reopen anchor: same derivation as launch. Gauntlet challenger is slot 0,
    // so excluding it as the "target" reproduces the live anchor choice.
    let target = (rec.format == "gauntlet")
        .then(|| players.first().cloned())
        .flatten();
    // No derivable anchor means no non-target player exists — a degenerate /
    // corrupt tournament shape. The live path errors at launch; surface the
    // same here instead of falling back to a non-participant `greedy`, which
    // `build_standings` would swallow as `AnchorNotFound` and blank every row
    // with no diagnostic.
    let anchor_id =
        tournament_config::derive_anchor(&players, target.as_deref()).ok_or_else(|| {
            format!(
                "tournament {tournament_id} has no derivable Elo anchor \
                 (need at least one non-target player); stored shape looks corrupt"
            )
        })?;
    let elo_options = tournament_config::elo_options(&anchor_id);
    let standings = build_standings(&st, &elo_options, &players, &anchor_id);

    let target_games = rec.target_games_per_matchup.unwrap_or(0);
    let max_failures = TournamentParams::from_json(&rec.params_json)
        .map(|p| p.max_failures_per_pair)
        .unwrap_or(0);
    let total = matchup_count(&rec.format, players.len()) as u32 * target_games;
    let done = success_count(&st);
    let finished = tournament_finished(
        &st,
        &rec.format,
        &players,
        &rec.game_config_id,
        target_games,
        max_failures,
    );

    Ok(StandingsSnapshot {
        tournament_id,
        name: rec.name,
        format: rec.format,
        anchor_id,
        finished,
        done,
        total,
        standings,
    })
}

/// Reopen one tournament as a complete read model. This is also the durable
/// reconciliation endpoint for a running tournament: event delivery keeps the
/// UI immediate, while this snapshot repairs any lifecycle event a lagging
/// broadcast receiver missed.
#[tauri::command]
#[specta::specta]
pub async fn get_tournament_snapshot(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    tournament_id: i64,
) -> Result<TournamentSnapshot, String> {
    let running = current_running_tournament(&state).await == Some(tournament_id);
    let store = open_store(&app)?;
    let g = store.lock();
    let tid = TournamentId(tournament_id);
    let rec = g
        .get_tournament(tid)
        .map_err(|e| format!("get tournament: {e}"))?
        .ok_or_else(|| format!("tournament {tournament_id} not found"))?;
    let players: Vec<String> = g
        .get_tournament_players(tid)
        .map_err(|e| format!("tournament players: {e}"))?
        .into_iter()
        .map(|p| p.player_id)
        .collect();
    let attempts = g
        .get_attempts(tid, None)
        .map_err(|e| format!("attempts: {e}"))?;
    let game_config = g
        .get_game_config(&rec.game_config_id)
        .map_err(|e| format!("game config: {e}"))?
        .ok_or_else(|| {
            format!(
                "tournament {tournament_id} references missing game config {}",
                rec.game_config_id
            )
        })?;
    drop(g);

    let mut tournament_state = TournamentState::empty(tid);
    for attempt in &attempts {
        tournament_state.fold_attempt(attempt);
    }

    let target = (rec.format == "gauntlet")
        .then(|| players.first().cloned())
        .flatten();
    let anchor_id =
        tournament_config::derive_anchor(&players, target.as_deref()).ok_or_else(|| {
            format!(
                "tournament {tournament_id} has no derivable Elo anchor \
             (need at least one non-target player); stored shape looks corrupt"
            )
        })?;
    let elo_options = tournament_config::elo_options(&anchor_id);
    let standings = build_standings(&tournament_state, &elo_options, &players, &anchor_id);

    let params = TournamentParams::from_json(&rec.params_json).unwrap_or(TournamentParams {
        max_failures_per_pair: 0,
        seat_policy: SeatPolicy::Legacy,
    });
    let games_per_matchup = rec.target_games_per_matchup.unwrap_or(0);
    let total = matchup_count(&rec.format, players.len()) as u32 * games_per_matchup;
    let success = success_count(&tournament_state);
    let failure = attempts
        .iter()
        .filter(|attempt| matches!(attempt.outcome, AttemptOutcome::Failure { .. }))
        .count() as u32;
    let finished = tournament_finished(
        &tournament_state,
        &rec.format,
        &players,
        &rec.game_config_id,
        games_per_matchup,
        params.max_failures_per_pair,
    );

    let mut games = Vec::with_capacity(success as usize);
    let mut failures = Vec::with_capacity(failure as usize);
    for attempt in &attempts {
        let rat_id = stored_rat_id(
            attempt.key.orientation,
            &attempt.key.player1_id,
            &attempt.key.player2_id,
        );
        match &attempt.outcome {
            AttemptOutcome::Success {
                player1_score,
                player2_score,
                ..
            } => games.push(StoredFinishedGame {
                attempt_id: attempt.id,
                match_id: attempt.key.match_id,
                player1_id: attempt.key.player1_id.clone(),
                player2_id: attempt.key.player2_id.clone(),
                repetition_index: attempt.key.repetition_index,
                rat_id,
                player1_score: *player1_score,
                player2_score: *player2_score,
            }),
            AttemptOutcome::Failure { failure_reason, .. } => {
                let (kind, timeout_phase, failing_player_id) = stored_failure_projection(
                    failure_reason,
                    attempt.key.orientation,
                    &attempt.key.player1_id,
                    &attempt.key.player2_id,
                );
                failures.push(StoredMatchFailure {
                    attempt_id: attempt.id,
                    match_id: attempt.key.match_id,
                    player1_id: attempt.key.player1_id.clone(),
                    player2_id: attempt.key.player2_id.clone(),
                    repetition_index: attempt.key.repetition_index,
                    rat_id,
                    failing_player_id,
                    kind,
                    timeout_phase,
                    reason: failure_reason.clone(),
                });
            },
        }
    }

    let last_finished_at = attempts
        .iter()
        .map(|attempt| attempt.finished_at.clone())
        .max();
    let field = match rec.format.as_str() {
        "gauntlet" => format!(
            "{} vs {}",
            target.as_deref().unwrap_or("target"),
            players.len().saturating_sub(1)
        ),
        _ => format!("all pairs of {}", players.len()),
    };
    let plan_summary = stored_plan_summary(
        &field,
        game_config.width,
        game_config.height,
        games_per_matchup,
        rec.methodology,
    );
    let methodology = rec.methodology.map(StoredTournamentMethodology::from);

    Ok(TournamentSnapshot {
        tournament_id,
        name: rec.name,
        format: rec.format,
        target,
        anchor_id,
        running,
        finished,
        paired: matches!(params.seat_policy, SeatPolicy::Paired),
        methodology,
        created_at: rec.created_at,
        last_finished_at,
        plan_summary,
        players,
        games_per_matchup,
        done: success,
        total,
        success,
        failure,
        standings,
        games,
        failures,
    })
}

/// Final-position board + verdict for one game, read from its `ReplayFile`.
/// Returns `Missing` (not an error) when the file is absent — a failed match
/// or a draw with no replay leaves no board, and the card shows that state.
#[tauri::command]
#[specta::specta]
pub async fn get_game_replay(
    app: tauri::AppHandle,
    tournament_id: i64,
    match_id: u64,
) -> Result<GameReplayState, String> {
    let dir = crate::tournament_paths::tournament_replay_dir(&app, tournament_id)?;
    let path = dir.join(format!("match-{match_id}.json"));
    let json = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => {
            return Ok(GameReplayState::Missing {
                reason: "no replay for this match".into(),
            })
        },
    };
    let replay: ReplayFile =
        serde_json::from_str(&json).map_err(|e| format!("parse replay {}: {e}", path.display()))?;
    Ok(project_final_state(&replay))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Project a `ReplayFile` to its final board + verdict. Geometry comes from the
/// match config; positions/scores/remaining cheese from the last `TurnPlayed`
/// (or the starting layout if zero turns were played).
fn project_final_state(replay: &ReplayFile) -> GameReplayState {
    let Some(cfg) = replay.match_config.as_ref() else {
        return GameReplayState::Missing {
            reason: "match did not start".into(),
        };
    };

    // Slot → agent_id from BotIdentified (player 0 = p1, 1 = p2).
    let mut player1_id = String::new();
    let mut player2_id = String::new();
    for ev in &replay.events {
        if let ReplayEvent::BotIdentified {
            player, agent_id, ..
        } = ev
        {
            match player {
                0 => player1_id = agent_id.clone(),
                1 => player2_id = agent_id.clone(),
                _ => {},
            }
        }
    }

    let last_turn = replay
        .events
        .iter()
        .rev()
        .find(|ev| matches!(ev, ReplayEvent::TurnPlayed { .. }));

    let (turn, p1, p2, cheese, state_hash) = match last_turn {
        Some(ReplayEvent::TurnPlayed {
            turn,
            p1_position,
            p2_position,
            p1_score,
            p2_score,
            p1_mud_turns,
            p2_mud_turns,
            cheese,
            state_hash,
            ..
        }) => (
            *turn,
            PlayerState {
                position: (*p1_position).into(),
                score: *p1_score,
                mud_turns: *p1_mud_turns,
            },
            PlayerState {
                position: (*p2_position).into(),
                score: *p2_score,
                mud_turns: *p2_mud_turns,
            },
            cheese.iter().map(|&c| c.into()).collect(),
            format!("{state_hash:016x}"),
        ),
        _ => (
            0,
            start_player(cfg.player1_start),
            start_player(cfg.player2_start),
            cfg.cheese.iter().map(|&c| c.into()).collect(),
            "0".to_string(),
        ),
    };

    let final_state = MazeState {
        width: cfg.width,
        height: cfg.height,
        turn,
        max_turns: cfg.max_turns,
        walls: cfg
            .walls
            .iter()
            .map(|&(a, b)| WallEntry {
                from: a.into(),
                to: b.into(),
            })
            .collect(),
        mud: cfg
            .mud
            .iter()
            .map(|&(a, b, cost)| MudEntry {
                from: a.into(),
                to: b.into(),
                cost,
            })
            .collect(),
        cheese,
        player1: p1,
        player2: p2,
        total_cheese: cfg.cheese.len() as u16,
        state_hash,
    };

    // result u8: 0 = player1, 1 = player2, 2 = draw.
    let winner = match replay.result.result {
        0 => Some(player1_id.clone()),
        1 => Some(player2_id.clone()),
        _ => None,
    };

    GameReplayState::Available {
        final_state,
        winner,
        player1_id,
        player2_id,
        player1_score: replay.result.player1_score,
        player2_score: replay.result.player2_score,
        turns: replay.result.turns_played,
    }
}

fn start_player(start: (u8, u8)) -> PlayerState {
    PlayerState {
        position: start.into(),
        score: 0.0,
        mud_turns: 0,
    }
}

fn stored_rat_id(orientation: SeatOrientation, player1_id: &str, player2_id: &str) -> String {
    match orientation {
        SeatOrientation::Canonical => player1_id.to_string(),
        SeatOrientation::Flipped => player2_id.to_string(),
    }
}

fn stored_failure_projection(
    reason: &str,
    orientation: SeatOrientation,
    player1_id: &str,
    player2_id: &str,
) -> (FailureKind, Option<TimeoutPhase>, Option<String>) {
    let (kind, timeout_phase) = if let Some(payload) = reason.strip_prefix("timeout: ") {
        let phase = payload.split(':').next().unwrap_or_default().trim();
        let phase = match phase {
            "setup" => Some(TimeoutPhase::Setup),
            "preprocessing" => Some(TimeoutPhase::Preprocessing),
            "sync" => Some(TimeoutPhase::Sync),
            "move" => Some(TimeoutPhase::Move),
            _ => None,
        };
        (FailureKind::Timeout, phase)
    } else if reason.starts_with("disconnected:") {
        (FailureKind::Disconnected, None)
    } else if reason == "spawn_failed" {
        (FailureKind::SpawnFailed, None)
    } else if reason == "handshake_timeout" {
        (FailureKind::HandshakeTimeout, None)
    } else if reason.starts_with("protocol_error:") {
        (FailureKind::ProtocolError, None)
    } else if reason == "cancelled" {
        (FailureKind::Cancelled, None)
    } else if reason == "panic"
        || reason.starts_with("sink_flush_error:")
        || reason.starts_with("internal:")
    {
        (FailureKind::Internal, None)
    } else {
        (FailureKind::Other, None)
    };

    // Only timeout/disconnect variants carry an implicated engine seat in the
    // durable format. Do not grep arbitrary payloads: a protocol/internal
    // message may mention "Player1" without assigning bot blame, and the live
    // event path deliberately leaves those structural failures unattributed.
    let implicated_slot = if reason.starts_with("timeout: ") || reason.starts_with("disconnected: ")
    {
        reason.rsplit_once(": ").map(|(_, slot)| slot)
    } else {
        None
    };

    // Apply the stored orientation once to map the engine seat back to the
    // canonical bot id.
    let (slot0, slot1) = orientation.canonicalize(player1_id, player2_id);
    let failing_player_id = if implicated_slot == Some("Player1") {
        Some(slot0.to_string())
    } else if implicated_slot == Some("Player2") {
        Some(slot1.to_string())
    } else {
        None
    };
    (kind, timeout_phase, failing_player_id)
}

fn open_store(app: &tauri::AppHandle) -> Result<Arc<Mutex<EvalStore>>, String> {
    let path = crate::tournament_paths::store_path(app)?;
    let store =
        EvalStore::open(&path).map_err(|e| format!("open store {}: {e}", path.display()))?;
    Ok(Arc::new(Mutex::new(store)))
}

fn resolve_players(bots: &[BotPick]) -> Vec<ResolvedPlayer> {
    bots.iter()
        .map(|b| ResolvedPlayer {
            id: b.agent_id.clone(),
            spec: PlayerSpec::Subprocess {
                agent_id: b.agent_id.clone(),
                command: b.run_command.clone(),
                working_dir: Some(PathBuf::from(&b.working_dir)),
            },
        })
        .collect()
}

/// "my-bot vs 5 (gauntlet) · 11×9 · 16 games/matchup · 200 ms/move" — the
/// provenance string, derived from the *configured* conditions (not pinned
/// constants), so it can never drift from what actually runs. Target namespace
/// is stripped for readability.
fn plan_summary(
    target: &Option<String>,
    player_count: usize,
    factory: &GameFactoryConfig,
    target_games: u32,
    move_timeout_ms: u32,
) -> String {
    let conditions = format!(
        "{}×{} · {target_games} games/matchup · {move_timeout_ms} ms/move",
        factory.width, factory.height
    );
    match target {
        Some(t) => {
            let label = t.rsplit('/').next().unwrap_or(t);
            format!(
                "{label} vs {} (gauntlet) · {conditions}",
                player_count.saturating_sub(1)
            )
        },
        None => format!("all pairs of {player_count} (round-robin) · {conditions}"),
    }
}

fn stored_plan_summary(
    field: &str,
    width: u32,
    height: u32,
    games_per_matchup: u32,
    methodology: Option<TournamentMethodology>,
) -> String {
    let base = format!("{field} · {width}×{height} · {games_per_matchup} games/matchup");
    match methodology {
        Some(methodology) => format!(
            "{base} · {} ms/move · {} ms/preprocess · {} concurrent",
            methodology.move_timeout_ms,
            methodology.preprocessing_timeout_ms,
            methodology.max_parallel
        ),
        None => format!("{base} · timing/concurrency not recorded"),
    }
}

fn matchup_count(format: &str, n: usize) -> usize {
    match format {
        "gauntlet" => n.saturating_sub(1),
        _ => n * n.saturating_sub(1) / 2,
    }
}

fn success_count(state: &TournamentState) -> u32 {
    state
        .history
        .values()
        .flat_map(|v| v.iter())
        .filter(|a| matches!(a.outcome, pyrat_eval::MatchupOutcome::Success { .. }))
        .count() as u32
}

/// True when every expected (pair, repetition) slot is done — a slot is done
/// if it has any success OR has hit `max_failures` failures (the planner's
/// `slot_done`). Counting successes-vs-target alone would mark a
/// failure-exhausted matchup as forever-unfinished.
fn tournament_finished(
    state: &TournamentState,
    format: &str,
    players: &[String],
    game_config_id: &str,
    target: u32,
    max_failures: u32,
) -> bool {
    if players.len() < 2 || target == 0 {
        return false;
    }
    let pairs = expected_pairs(format, players);
    pairs.iter().all(|(a, b)| {
        (0..target).all(|rep| {
            let key = MatchupKey::from_pair(a, b, game_config_id, rep);
            slot_done(state, &key, max_failures)
        })
    })
}

fn slot_done(state: &TournamentState, key: &MatchupKey, max_failures: u32) -> bool {
    let attempts = state.history.get(key);
    let Some(attempts) = attempts else {
        return false;
    };
    let mut failures = 0u32;
    for a in attempts {
        match a.outcome {
            pyrat_eval::MatchupOutcome::Success { .. } => return true,
            pyrat_eval::MatchupOutcome::Failure => failures += 1,
        }
    }
    failures >= max_failures
}

fn expected_pairs(format: &str, players: &[String]) -> Vec<(String, String)> {
    match format {
        "gauntlet" => {
            // Slot 0 is the challenger; it plays each opponent.
            let challenger = &players[0];
            players[1..]
                .iter()
                .map(|opp| (challenger.clone(), opp.clone()))
                .collect()
        },
        _ => {
            let mut pairs = Vec::new();
            for i in 0..players.len() {
                for j in (i + 1)..players.len() {
                    pairs.push((players[i].clone(), players[j].clone()));
                }
            }
            pairs
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded_methodology() -> TournamentMethodology {
        TournamentMethodology {
            timing_mode: TournamentTimingMode::Wait,
            move_timeout_ms: 200,
            preprocessing_timeout_ms: 2_000,
            startup_timeout_ms: 120_000,
            configure_timeout_ms: 5_000,
            network_grace_ms: 50,
            max_parallel: 4,
        }
    }

    fn pick(agent_id: &str, working_dir: &str) -> BotPick {
        BotPick {
            agent_id: agent_id.to_string(),
            run_command: "cargo run".to_string(),
            working_dir: working_dir.to_string(),
        }
    }

    #[test]
    fn warmup_dedupes_by_agent_id_first_seen() {
        // Round-robin lists the same bot in many picks; warmup must build it
        // once. Dedup is by agent_id, keeping first-seen order.
        let bots = vec![pick("a", "/a"), pick("b", "/b"), pick("a", "/a-dup")];
        let distinct = distinct_by_agent(&bots);
        assert_eq!(distinct.len(), 2);
        assert_eq!(distinct[0].agent_id, "a");
        assert_eq!(distinct[0].working_dir, "/a"); // first-seen wins
        assert_eq!(distinct[1].agent_id, "b");
    }

    /// The launch-blocker: a tournament that *finished* leaves its slot
    /// `Running` (the runner never resets the phase), and the next launch must
    /// still be allowed. Idle is free, in-flight Starting/Running are refused.
    #[tokio::test]
    async fn finished_running_slot_is_reclaimable() {
        assert!(reservation_check(&TournamentPhase::Idle).is_ok());
        assert!(reservation_check(&TournamentPhase::Starting {
            cancel: CancellationToken::new(),
        })
        .is_err());

        // A still-running tournament: refused.
        let live = tokio::spawn(std::future::pending::<()>());
        let running_live = TournamentPhase::Running {
            tournament_id: 1,
            cancel: CancellationToken::new(),
            handle: live,
        };
        assert!(reservation_check(&running_live).is_err());

        // A finished tournament's slot: reclaimable (the bug — otherwise no
        // further launch is ever possible).
        let done = tokio::spawn(async {});
        while !done.is_finished() {
            tokio::task::yield_now().await;
        }
        let running_done = TournamentPhase::Running {
            tournament_id: 1,
            cancel: CancellationToken::new(),
            handle: done,
        };
        assert!(reservation_check(&running_done).is_ok());
    }

    /// 7×7, no walls/mud, random symmetric — the interesting validation case.
    fn random_symmetric_factory() -> GameFactoryConfig {
        GameFactoryConfig {
            width: 7,
            height: 7,
            max_turns: 100,
            wall_density: 0.0,
            mud_density: 0.0,
            mud_range: 2,
            connected: true,
            symmetric: true,
            player_start: PlayerStart::Random,
            cheese_count: 4,
            cheese_symmetric: true,
        }
    }

    #[test]
    fn random_symmetric_odd_cheese_rejected() {
        let f = GameFactoryConfig {
            cheese_count: 5,
            ..random_symmetric_factory()
        };
        let Err(err) = f.to_game_config() else {
            panic!("odd symmetric cheese with random starts should be rejected");
        };
        assert!(err.contains("even cheese count"), "{err}");
    }

    #[test]
    fn random_symmetric_even_cheese_over_capacity_rejected() {
        // 7×7 odd board → cap = 49 - 5 = 44; 46 is over.
        let f = GameFactoryConfig {
            cheese_count: 46,
            ..random_symmetric_factory()
        };
        let Err(err) = f.to_game_config() else {
            panic!("over-capacity random symmetric cheese should be rejected");
        };
        assert!(err.contains("over capacity"), "{err}");
    }

    #[test]
    fn random_symmetric_even_within_capacity_builds_every_seed() {
        // The capacity cap exists so no *seed* can fail mid-tournament: sweep.
        let f = GameFactoryConfig {
            cheese_count: 10,
            ..random_symmetric_factory()
        };
        let cfg = f.to_game_config().expect("should build");
        for seed in 0..30u64 {
            cfg.create(Some(seed))
                .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        }
    }

    #[test]
    fn out_of_range_dims_rejected() {
        for (w, h) in [(1, 7), (300, 7), (7, 1)] {
            let f = GameFactoryConfig {
                width: w,
                height: h,
                ..random_symmetric_factory()
            };
            assert!(f.to_game_config().is_err(), "{w}x{h} should be rejected");
        }
    }

    #[test]
    fn launch_defaults_roundtrip_through_factory() {
        // The default (tiny preset) must be representable as a factory and
        // rebuildable — the no-stale-mirror guarantee for `LaunchDefaults`.
        let cfg = tournament_config::game_config().unwrap();
        let factory = factory_from_game_config(&cfg).unwrap();
        factory
            .to_game_config()
            .unwrap()
            .create(Some(7))
            .expect("default factory builds");
        assert!(matches!(factory.player_start, PlayerStart::Corners));
    }

    #[test]
    fn stored_methodology_projection_preserves_every_execution_condition() {
        let projected = StoredTournamentMethodology::from(recorded_methodology());
        assert_eq!(projected.timing_mode, StoredTimingMode::Wait);
        assert_eq!(projected.move_timeout_ms, 200);
        assert_eq!(projected.preprocessing_timeout_ms, 2_000);
        assert_eq!(projected.startup_timeout_ms, 120_000);
        assert_eq!(projected.configure_timeout_ms, 5_000);
        assert_eq!(projected.network_grace_ms, 50);
        assert_eq!(projected.max_parallel, 4);
    }

    #[test]
    fn stored_plan_summary_distinguishes_recorded_from_legacy_unknown() {
        let recorded =
            stored_plan_summary("all pairs of 3", 11, 9, 16, Some(recorded_methodology()));
        assert_eq!(
            recorded,
            "all pairs of 3 · 11×9 · 16 games/matchup · 200 ms/move · \
             2000 ms/preprocess · 4 concurrent"
        );

        let legacy = stored_plan_summary("all pairs of 3", 11, 9, 16, None);
        assert_eq!(
            legacy,
            "all pairs of 3 · 11×9 · 16 games/matchup · timing/concurrency not recorded"
        );
    }

    #[test]
    fn stored_failure_projection_restores_timeout_phase_and_canonical_bot() {
        let (kind, phase, bot) = stored_failure_projection(
            "timeout: move: Player1",
            SeatOrientation::Canonical,
            "alice",
            "bob",
        );
        assert!(matches!(kind, FailureKind::Timeout));
        assert!(matches!(phase, Some(TimeoutPhase::Move)));
        assert_eq!(bot.as_deref(), Some("alice"));

        // In a flipped leg, engine Player1 is the canonical second bot.
        let (kind, phase, bot) = stored_failure_projection(
            "timeout: preprocessing: Player1",
            SeatOrientation::Flipped,
            "alice",
            "bob",
        );
        assert!(matches!(kind, FailureKind::Timeout));
        assert!(matches!(phase, Some(TimeoutPhase::Preprocessing)));
        assert_eq!(bot.as_deref(), Some("bob"));

        let (kind, phase, bot) = stored_failure_projection(
            "disconnected: Player2",
            SeatOrientation::Flipped,
            "alice",
            "bob",
        );
        assert!(matches!(kind, FailureKind::Disconnected));
        assert!(phase.is_none());
        assert_eq!(bot.as_deref(), Some("alice"));
    }

    #[test]
    fn stored_failure_projection_does_not_invent_blame_from_payload_text() {
        let (kind, phase, bot) = stored_failure_projection(
            "protocol_error: expected Ready from Player1",
            SeatOrientation::Flipped,
            "alice",
            "bob",
        );
        assert!(matches!(kind, FailureKind::ProtocolError));
        assert!(phase.is_none());
        assert_eq!(bot, None);

        let (kind, phase, bot) = stored_failure_projection(
            "internal: Player2 channel bookkeeping failed",
            SeatOrientation::Canonical,
            "alice",
            "bob",
        );
        assert!(matches!(kind, FailureKind::Internal));
        assert!(phase.is_none());
        assert_eq!(bot, None);
    }

    #[test]
    fn stored_rat_id_tracks_the_actual_seat_across_a_pair() {
        assert_eq!(
            stored_rat_id(SeatOrientation::Canonical, "alice", "bob"),
            "alice"
        );
        assert_eq!(
            stored_rat_id(SeatOrientation::Flipped, "alice", "bob"),
            "bob"
        );
    }

    #[test]
    fn stored_completion_uses_slot_semantics_not_success_count() {
        let players = vec!["alice".to_string(), "bob".to_string()];
        let config_id = "cfg";
        let mut state = TournamentState::empty(TournamentId(1));
        state.history.insert(
            MatchupKey::from_pair("alice", "bob", config_id, 0),
            vec![pyrat_eval::MatchupAttempt {
                attempt_index: 0,
                outcome: pyrat_eval::MatchupOutcome::Success {
                    player1_score: 5.0,
                    player2_score: 3.0,
                },
            }],
        );
        state.history.insert(
            MatchupKey::from_pair("alice", "bob", config_id, 1),
            vec![pyrat_eval::MatchupAttempt {
                attempt_index: 0,
                outcome: pyrat_eval::MatchupOutcome::Failure,
            }],
        );

        // The historical snapshot reports one scored game, but the paired
        // two-slot schedule is terminal because the other slot exhausted its
        // failure allowance. It must not remain "running" forever merely
        // because success_count is below the planned slot count.
        assert_eq!(success_count(&state), 1);
        assert!(tournament_finished(
            &state,
            "round_robin",
            &players,
            config_id,
            2,
            1,
        ));
        assert!(!tournament_finished(
            &state,
            "round_robin",
            &players,
            config_id,
            2,
            2,
        ));
    }
}
