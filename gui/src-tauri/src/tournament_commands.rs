//! Tauri commands for the tournament module.
//!
//! `start_tournament` creates the row (returning its id) and spawns the
//! runner on a background task. The store is the same SQLite the CLI uses, at
//! the app-data path (`tournament_paths`). Read commands open a fresh
//! connection each call — WAL allows concurrent readers, and they're
//! infrequent.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use pyrat::game::builder::{CheeseStrategy, GameConfig, MazeParams, MazeStrategy, PlayerStrategy};
use pyrat_eval::{
    EvalSession, MatchupKey, ResolvedPlayer, SeatPolicy, TournamentMethodology, TournamentParams,
    TournamentSpec, TournamentState, TournamentTimingMode,
};
use pyrat_eval_store::{
    AttemptFailureReport, AttemptOutcome, AttemptRecord, EvalStore, SeatOrientation, TournamentId,
    TournamentLifecycle, TournamentRecord, TournamentTerminalKind,
};
use pyrat_host::probe::{preflight_bot_in_slot, PreflightConfig};
use pyrat_host::wire::Player as PlayerSlot;
use pyrat_orchestrator::{PlayerSpec, ReplayEvent, ReplayFile};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;
use tracing::warn;

use crate::commands::{MazeState, MudEntry, PlayerState, WallEntry};
use crate::state::{
    AppState, TournamentControl, TournamentLease, TournamentPhase, TournamentStopCause,
    TournamentSupervisorCompletion,
};
use crate::tournament_config;
use crate::tournament_events::{
    FailureKind, RatingReadiness, StandingRow, TimeoutPhase, TournamentAbortedEvent,
    TournamentFinishedEvent, TournamentLifecycleStatus, TournamentPreparingEvent,
    TournamentProgress, TournamentRuntimePhase, TournamentRuntimeStatus, TournamentStartedEvent,
    TournamentStoppingEvent, TournamentTerminalState,
};
use crate::tournament_runner::{
    build_standings, cancellation_outcome, failed_outcome, ordered_player_ids,
    persist_outcome_once, run_tournament, RunnerFormat, TournamentRun, TournamentRunOutcome,
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

const MAX_PARTICIPANTS: u32 = 256;
const MAX_MAZES_PER_MATCHUP: u32 = 50_000;
const MAX_TOTAL_GAMES: u32 = 100_000;
const MAX_TIMEOUT_MS: u32 = 3_600_000;
const MAX_PARALLEL: u32 = 64;

/// Backend-owned bounds for launch controls. The frontend uses these for
/// affordances; the validation boundary below remains authoritative.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct LaunchLimits {
    pub min_dimension: u32,
    pub max_dimension: u32,
    pub max_turns: u32,
    pub max_mud_range: u32,
    pub max_cheese_count: u32,
    pub max_participants: u32,
    pub max_mazes_per_matchup: u32,
    pub max_total_games: u32,
    pub max_timeout_ms: u32,
    pub max_parallel: u32,
    pub max_js_safe_seed: u64,
}

impl Default for LaunchLimits {
    fn default() -> Self {
        Self {
            min_dimension: u32::from(GameConfig::MIN_DIMENSION),
            max_dimension: u32::from(GameConfig::MAX_DIMENSION),
            max_turns: u32::from(GameConfig::MAX_MAX_TURNS),
            max_mud_range: u32::from(GameConfig::MAX_MUD_COST),
            max_cheese_count: u32::from(GameConfig::MAX_CHEESE_COUNT),
            max_participants: MAX_PARTICIPANTS,
            max_mazes_per_matchup: MAX_MAZES_PER_MATCHUP,
            max_total_games: MAX_TOTAL_GAMES,
            max_timeout_ms: MAX_TIMEOUT_MS,
            max_parallel: MAX_PARALLEL,
            max_js_safe_seed: MAX_JS_SAFE_SEED,
        }
    }
}

/// One field-addressable launch validation failure.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct LaunchFieldError {
    pub field: String,
    pub message: String,
}

impl LaunchFieldError {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for LaunchFieldError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// Start errors keep form mistakes structured while preserving ordinary
/// runtime failures as a single message.
#[derive(Serialize, Debug, Clone, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartTournamentError {
    Validation { errors: Vec<LaunchFieldError> },
    Runtime { message: String },
}

impl StartTournamentError {
    fn validation(errors: Vec<LaunchFieldError>) -> Self {
        Self::Validation { errors }
    }

    fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime {
            message: message.into(),
        }
    }
}

impl From<String> for StartTournamentError {
    fn from(message: String) -> Self {
        Self::runtime(message)
    }
}

impl From<&str> for StartTournamentError {
    fn from(message: &str) -> Self {
        Self::runtime(message)
    }
}

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
/// Integer-like wire fields intentionally arrive as `f64`: JavaScript has one
/// numeric type, and accepting the raw value lets the command return a typed
/// field error for `7.5` instead of failing in Tauri deserialization.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct GameFactoryConfig {
    pub width: f64,
    pub height: f64,
    pub max_turns: f64,
    pub wall_density: f64,
    pub mud_density: f64,
    pub mud_range: f64,
    pub connected: bool,
    pub symmetric: bool,
    pub player_start: PlayerStart,
    pub cheese_count: f64,
    pub cheese_symmetric: bool,
}

impl GameFactoryConfig {
    /// Build the engine `GameConfig` through its authoritative checked
    /// constructor. No untrusted launch value reaches a narrowing cast first.
    fn to_game_config(&self) -> Result<GameConfig, LaunchFieldError> {
        let width = validate_whole_number(
            "width",
            self.width,
            u64::from(GameConfig::MIN_DIMENSION),
            u64::from(GameConfig::MAX_DIMENSION),
        )? as u8;
        let height = validate_whole_number(
            "height",
            self.height,
            u64::from(GameConfig::MIN_DIMENSION),
            u64::from(GameConfig::MAX_DIMENSION),
        )? as u8;
        let max_turns = validate_whole_number(
            "max_turns",
            self.max_turns,
            u64::from(GameConfig::MIN_MAX_TURNS),
            u64::from(GameConfig::MAX_MAX_TURNS),
        )? as u16;
        let mud_range = validate_whole_number(
            "mud_range",
            self.mud_range,
            u64::from(GameConfig::MIN_MUD_COST),
            u64::from(GameConfig::MAX_MUD_COST),
        )? as u8;
        let cheese_count = validate_whole_number(
            "cheese_count",
            self.cheese_count,
            1,
            u64::from(GameConfig::MAX_CHEESE_COUNT),
        )? as u16;

        if self.player_start == PlayerStart::Random && self.cheese_symmetric {
            if cheese_count % 2 == 1 {
                return Err(LaunchFieldError::new(
                    "cheese_count",
                    "random starts with symmetric cheese need an even cheese count",
                ));
            }
            let area = u16::from(width) * u16::from(height);
            let odd_board = !width.is_multiple_of(2) && !height.is_multiple_of(2);
            let capacity = area - if odd_board { 5 } else { 4 };
            if cheese_count > capacity {
                return Err(LaunchFieldError::new(
                    "cheese_count",
                    format!(
                        "cheese_count over capacity for random starts with symmetric cheese: max {capacity}, got {cheese_count}"
                    ),
                ));
            }
        }

        let maze = MazeParams {
            wall_density: self.wall_density as f32,
            connected: self.connected,
            symmetric: self.symmetric,
            mud_density: self.mud_density as f32,
            mud_range,
        };
        let players = match self.player_start {
            PlayerStart::Corners => PlayerStrategy::Corners,
            PlayerStart::Random => PlayerStrategy::Random,
        };
        GameConfig::try_from_parts(
            width,
            height,
            max_turns,
            MazeStrategy::Random(maze),
            players,
            CheeseStrategy::Random {
                count: cheese_count,
                symmetric: self.cheese_symmetric,
            },
        )
        .map_err(|error| LaunchFieldError::new(error.field(), error.to_string()))
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
        width: f64::from(cfg.width()),
        height: f64::from(cfg.height()),
        max_turns: f64::from(cfg.max_turns()),
        wall_density: f64::from(p.wall_density),
        mud_density: f64::from(p.mud_density),
        mud_range: f64::from(p.mud_range),
        connected: p.connected,
        symmetric: p.symmetric,
        player_start,
        cheese_count: f64::from(*count),
        cheese_symmetric: *symmetric,
    })
}

/// Launch parameters. The factory + methodology knobs are configured on the
/// launch screen; the frontend pre-fills them from `get_tournament_launch_defaults`.
/// Methodology numbers likewise stay raw until `validate_launch_params`
/// proves they are finite whole values in range and narrows them to `u32`.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct LaunchParams {
    pub bots: Vec<BotPick>,
    /// The starred bot to measure → gauntlet. `None` → round-robin.
    pub target: Option<String>,
    pub name: Option<String>,
    /// The game-instance distribution (board / maze / starts / cheese).
    pub factory: GameFactoryConfig,
    /// Mazes per matchup; the paired schedule runs 2× this many games.
    pub mazes_per_matchup: f64,
    pub move_timeout_ms: f64,
    pub preprocessing_timeout_ms: f64,
    pub max_parallel: f64,
    /// Tournament seed (selects which instances are drawn). `None` → random,
    /// capped to the JS-safe range so it round-trips for reproducibility.
    pub tournament_seed: Option<f64>,
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
    pub limits: LaunchLimits,
}

struct ValidatedLaunch {
    game_config: GameConfig,
    target_games_per_matchup: u32,
    total_games: u32,
    move_timeout_ms: u32,
    preprocessing_timeout_ms: u32,
    max_parallel: u32,
    tournament_seed: Option<u64>,
}

fn validate_whole_number(
    field: &'static str,
    actual: f64,
    min: u64,
    max: u64,
) -> Result<u64, LaunchFieldError> {
    if !actual.is_finite() || actual.fract() != 0.0 {
        return Err(LaunchFieldError::new(
            field,
            format!("{field} must be a finite whole number"),
        ));
    }
    if actual < min as f64 || actual > max as f64 {
        return Err(LaunchFieldError::new(
            field,
            format!("{field} must be {min}..={max}, got {actual}"),
        ));
    }
    Ok(actual as u64)
}

/// Validate every externally supplied launch value before reserving the
/// runner slot or touching durable state. All failures are field-addressable
/// so the frontend can put the explanation beside the responsible control.
fn validate_launch_params(params: &LaunchParams) -> Result<ValidatedLaunch, Vec<LaunchFieldError>> {
    let mut errors = Vec::new();

    let game_config = match params.factory.to_game_config() {
        Ok(config) => Some(config),
        Err(error) => {
            errors.push(LaunchFieldError::new(
                format!("factory.{}", error.field),
                error.message,
            ));
            None
        },
    };

    if params.bots.len() < 2 {
        errors.push(LaunchFieldError::new(
            "bots",
            "a tournament needs at least 2 bots",
        ));
    }
    if params.bots.len() > MAX_PARTICIPANTS as usize {
        errors.push(LaunchFieldError::new(
            "bots",
            format!("at most {MAX_PARTICIPANTS} bots are allowed"),
        ));
    }

    let mut ids = HashSet::new();
    for (index, bot) in params.bots.iter().enumerate() {
        let id_field = format!("bots.{index}.agent_id");
        if bot.agent_id.trim().is_empty() {
            errors.push(LaunchFieldError::new(id_field, "bot id must not be empty"));
        } else if bot.agent_id.trim() != bot.agent_id {
            errors.push(LaunchFieldError::new(
                id_field,
                "bot id must not start or end with whitespace",
            ));
        } else if !ids.insert(bot.agent_id.as_str()) {
            errors.push(LaunchFieldError::new(
                id_field,
                format!("duplicate bot id `{}`", bot.agent_id),
            ));
        }

        if bot.run_command.trim().is_empty() {
            errors.push(LaunchFieldError::new(
                format!("bots.{index}.run_command"),
                "run command must not be empty",
            ));
        }

        let working_dir_field = format!("bots.{index}.working_dir");
        if bot.working_dir.trim().is_empty() {
            errors.push(LaunchFieldError::new(
                working_dir_field,
                "working directory must not be empty",
            ));
        } else if !Path::new(&bot.working_dir).is_dir() {
            errors.push(LaunchFieldError::new(
                working_dir_field,
                "working directory does not exist or is not a directory",
            ));
        }
    }

    if let Some(target) = &params.target {
        if target.trim().is_empty() {
            errors.push(LaunchFieldError::new(
                "target",
                "target bot must not be empty",
            ));
        } else {
            let target_count = params
                .bots
                .iter()
                .filter(|bot| bot.agent_id == *target)
                .count();
            if target_count != 1 {
                errors.push(LaunchFieldError::new(
                    "target",
                    format!("target `{target}` must identify exactly one selected bot"),
                ));
            }
        }
    }

    let mazes_per_matchup = match validate_whole_number(
        "mazes_per_matchup",
        params.mazes_per_matchup,
        1,
        u64::from(MAX_MAZES_PER_MATCHUP),
    ) {
        Ok(value) => Some(value as u32),
        Err(error) => {
            errors.push(error);
            None
        },
    };
    let target_games_per_matchup = mazes_per_matchup.and_then(|mazes| match mazes.checked_mul(2) {
        Some(value) => Some(value),
        None => {
            errors.push(LaunchFieldError::new(
                "mazes_per_matchup",
                "paired game count overflows",
            ));
            None
        },
    });

    let mut checked_u32 =
        |field: &'static str, value: f64, min: u32, max: u32| match validate_whole_number(
            field,
            value,
            u64::from(min),
            u64::from(max),
        ) {
            Ok(value) => Some(value as u32),
            Err(error) => {
                errors.push(error);
                None
            },
        };
    let move_timeout_ms = checked_u32("move_timeout_ms", params.move_timeout_ms, 1, MAX_TIMEOUT_MS);
    let preprocessing_timeout_ms = checked_u32(
        "preprocessing_timeout_ms",
        params.preprocessing_timeout_ms,
        1,
        MAX_TIMEOUT_MS,
    );
    let max_parallel = checked_u32("max_parallel", params.max_parallel, 1, MAX_PARALLEL);

    let tournament_seed = match params.tournament_seed {
        Some(seed) => match validate_whole_number("tournament_seed", seed, 0, MAX_JS_SAFE_SEED) {
            Ok(seed) => Some(Some(seed)),
            Err(error) => {
                errors.push(error);
                None
            },
        },
        None => Some(None),
    };

    let total_games = target_games_per_matchup.and_then(|games_per_matchup| {
        let player_count = u32::try_from(params.bots.len()).ok()?;
        let matchup_count = if params.target.is_some() {
            player_count.checked_sub(1)
        } else {
            player_count
                .checked_mul(player_count.checked_sub(1)?)?
                .checked_div(2)
        };
        let total = matchup_count.and_then(|count| count.checked_mul(games_per_matchup));
        match total {
            Some(total) if total <= MAX_TOTAL_GAMES => Some(total),
            Some(total) => {
                errors.push(LaunchFieldError::new(
                    "mazes_per_matchup",
                    format!(
                        "this schedule would run {total} games; the limit is {MAX_TOTAL_GAMES}"
                    ),
                ));
                None
            },
            None => {
                errors.push(LaunchFieldError::new(
                    "mazes_per_matchup",
                    "total game count overflows",
                ));
                None
            },
        }
    });

    if !errors.is_empty() {
        return Err(errors);
    }

    match (
        game_config,
        target_games_per_matchup,
        total_games,
        move_timeout_ms,
        preprocessing_timeout_ms,
        max_parallel,
        tournament_seed,
    ) {
        (
            Some(game_config),
            Some(target_games_per_matchup),
            Some(total_games),
            Some(move_timeout_ms),
            Some(preprocessing_timeout_ms),
            Some(max_parallel),
            Some(tournament_seed),
        ) => Ok(ValidatedLaunch {
            game_config,
            target_games_per_matchup,
            total_games,
            move_timeout_ms,
            preprocessing_timeout_ms,
            max_parallel,
            tournament_seed,
        }),
        _ => Err(vec![LaunchFieldError::new(
            "launch",
            "launch validation could not be completed",
        )]),
    }
}

/// Row in the "in this store" panel.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct TournamentSummary {
    pub id: i64,
    pub name: Option<String>,
    pub format: String,
    pub created_at: String,
    /// The app-owned runner is currently executing this tournament. Separate
    /// from durable lifecycle: navigation never owns runner lifetime.
    pub running: bool,
    pub lifecycle: TournamentLifecycleStatus,
    pub terminal: Option<TournamentTerminalState>,
    pub rating_readiness: RatingReadiness,
    pub rating_reason: Option<String>,
    pub progress: TournamentProgress,
}

/// Standings for a finished/partial tournament, reopened from the store.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct StandingsSnapshot {
    pub tournament_id: i64,
    pub name: Option<String>,
    pub format: String,
    pub anchor_id: String,
    pub lifecycle: TournamentLifecycleStatus,
    pub terminal: Option<TournamentTerminalState>,
    pub rating_readiness: RatingReadiness,
    pub rating_reason: Option<String>,
    pub progress: TournamentProgress,
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
    pub attempt_index: u32,
    pub rat_id: String,
    pub failing_player_id: Option<String>,
    pub kind: FailureKind,
    pub timeout_phase: Option<TimeoutPhase>,
    pub reason: String,
    /// This failure spent the slot's final retry budget. Earlier failures on
    /// the same slot remain inspectable but are not terminal legs.
    pub exhausted: bool,
}

/// Durable state of one expected schedule leg. `Running` is expressible for
/// live projections; store-only snapshots normally return `Pending` until a
/// terminal attempt lands. Old rows never have missing legs relabeled as
/// pending because their lifecycle is unknowable.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StoredSlotState {
    Successful {
        attempt_id: i64,
        match_id: Option<u64>,
    },
    Exhausted {
        last_failure_attempt_id: i64,
        failed_attempts: u32,
    },
    Pending,
    Running {
        match_id: u64,
    },
    LegacyUnknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct StoredTournamentSlot {
    pub player1_id: String,
    pub player2_id: String,
    pub repetition_index: u32,
    pub rat_id: String,
    pub state: StoredSlotState,
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
    pub lifecycle: TournamentLifecycleStatus,
    pub terminal: Option<TournamentTerminalState>,
    pub rating_readiness: RatingReadiness,
    pub rating_reason: Option<String>,
    pub paired: bool,
    /// Absent only when an older tournament row did not record its execution
    /// conditions. Optional in TypeScript so existing fixture consumers remain
    /// compatible; current rows always include it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub methodology: Option<StoredTournamentMethodology>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub terminal_at: Option<String>,
    pub last_finished_at: Option<String>,
    pub plan_summary: String,
    pub players: Vec<String>,
    pub games_per_matchup: u32,
    pub progress: TournamentProgress,
    pub standings: Vec<StandingRow>,
    pub games: Vec<StoredFinishedGame>,
    pub failures: Vec<StoredMatchFailure>,
    pub slots: Vec<StoredTournamentSlot>,
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

fn reservation_check(phase: &TournamentPhase) -> Result<(), String> {
    match phase {
        TournamentPhase::Idle => Ok(()),
        TournamentPhase::Starting(_) => Err("a tournament is already starting".into()),
        TournamentPhase::Running(_) => Err("a tournament is already running".into()),
        TournamentPhase::Stopping(_) => Err("the previous tournament is still stopping".into()),
    }
}

fn runtime_status(phase: &TournamentPhase) -> TournamentRuntimeStatus {
    match phase {
        TournamentPhase::Idle => TournamentRuntimeStatus {
            phase: TournamentRuntimePhase::Idle,
            tournament_id: None,
        },
        TournamentPhase::Starting(lease) => TournamentRuntimeStatus {
            phase: TournamentRuntimePhase::Starting,
            tournament_id: lease.tournament_id,
        },
        TournamentPhase::Running(lease) => TournamentRuntimeStatus {
            phase: TournamentRuntimePhase::Running,
            tournament_id: lease.tournament_id,
        },
        TournamentPhase::Stopping(lease) => TournamentRuntimeStatus {
            phase: TournamentRuntimePhase::Stopping,
            tournament_id: lease.tournament_id,
        },
    }
}

async fn current_tournament_runtime(state: &AppState) -> TournamentRuntimeStatus {
    let phase = state.tournament_phase.lock().await;
    runtime_status(&phase)
}

/// Reserve one generation and return only after its session is genuinely
/// live. The returned snapshot is authoritative; the matching event is
/// enrichment for other windows and background navigation.
#[tauri::command]
#[specta::specta]
pub async fn start_tournament(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    params: LaunchParams,
) -> Result<TournamentStartedEvent, StartTournamentError> {
    let validated = validate_launch_params(&params).map_err(StartTournamentError::validation)?;
    let generation = state
        .next_tournament_generation
        .fetch_add(1, Ordering::Relaxed);
    let control = TournamentControl::new();
    let (completion_tx, completion_rx) = tokio::sync::watch::channel(None);
    let (startup_tx, startup_rx) = tokio::sync::oneshot::channel();

    {
        let mut phase = state.tournament_phase.lock().await;
        reservation_check(&phase)?;
        *phase = TournamentPhase::Starting(TournamentLease {
            generation,
            tournament_id: None,
            control: control.clone(),
            completion: completion_rx,
        });
    }

    let phase = state.tournament_phase.clone();
    tokio::spawn(supervise_tournament(
        app,
        phase,
        generation,
        params,
        validated,
        control,
        completion_tx,
        startup_tx,
    ));

    match startup_rx.await {
        Ok(Ok(started)) => Ok(started),
        Ok(Err(error)) => Err(StartTournamentError::runtime(error)),
        Err(_) => Err(StartTournamentError::runtime(
            "tournament supervisor exited before startup completed",
        )),
    }
}

#[allow(clippy::too_many_arguments)]
async fn supervise_tournament(
    app: tauri::AppHandle,
    phase: Arc<tokio::sync::Mutex<TournamentPhase>>,
    generation: u64,
    params: LaunchParams,
    validated: ValidatedLaunch,
    control: TournamentControl,
    completion: tokio::sync::watch::Sender<Option<TournamentSupervisorCompletion>>,
    startup: tokio::sync::oneshot::Sender<Result<TournamentStartedEvent, String>>,
) {
    let worker_app = app.clone();
    let worker_phase = phase.clone();
    let worker_control = control.clone();
    let worker = tokio::spawn(async move {
        let (store, run) = match prepare_tournament(
            &worker_app,
            params,
            validated,
            worker_phase.clone(),
            generation,
            worker_control.clone(),
        )
        .await
        {
            Ok(prepared) => prepared,
            Err(reason) => {
                let _ = startup.send(Err(reason.clone()));
                return Err(reason);
            },
        };
        run_tournament(
            worker_app,
            store,
            run,
            worker_phase,
            generation,
            worker_control,
            startup,
        )
        .await
    });

    let worker_result = match worker.await {
        Ok(result) => result,
        Err(error) => Err(format!("tournament worker panicked: {error}")),
    };
    let tournament_id = {
        let phase = phase.lock().await;
        phase
            .lease()
            .filter(|lease| lease.generation == generation)
            .and_then(|lease| lease.tournament_id)
    };

    let outcome = match worker_result {
        Ok(outcome) => Ok(outcome),
        Err(reason) => match control.stop_cause() {
            Some(cause) => Ok(cancellation_outcome(cause)),
            None => Err(reason),
        },
    };
    let final_result = match (tournament_id, outcome) {
        (Some(tournament_id), Ok(outcome)) => {
            finalize_tournament(&app, TournamentId(tournament_id), &outcome)
        },
        (Some(tournament_id), Err(reason)) => {
            finalize_tournament(&app, TournamentId(tournament_id), &failed_outcome(reason))
        },
        (None, Ok(_)) => Ok(()),
        (None, Err(reason)) => Err(reason),
    };

    {
        let mut phase = phase.lock().await;
        phase.clear_if_generation(generation);
    }
    completion.send_replace(Some(TournamentSupervisorCompletion {
        generation,
        tournament_id,
        result: final_result.clone(),
    }));
    if let Err(error) = final_result {
        warn!(generation, error = %error, "tournament supervisor exited with error");
    }
}

fn finalize_tournament(
    app: &tauri::AppHandle,
    tournament_id: TournamentId,
    outcome: &TournamentRunOutcome,
) -> Result<(), String> {
    let store = open_store(app)?;
    let Some(terminal) = persist_outcome_once(&store, tournament_id, outcome)? else {
        return Ok(());
    };
    match outcome.lifecycle {
        TournamentLifecycle::Completed => {
            let _ = TournamentFinishedEvent {
                tournament_id: tournament_id.0,
                terminal,
                rating_readiness: outcome.rating_status.into(),
            }
            .emit(app);
        },
        TournamentLifecycle::Stopped | TournamentLifecycle::Failed => {
            let _ = TournamentAbortedEvent {
                tournament_id: tournament_id.0,
                lifecycle: outcome.lifecycle.into(),
                reason: outcome
                    .reason
                    .clone()
                    .unwrap_or_else(|| "tournament ended without a reason".into()),
                terminal,
                rating_readiness: outcome.rating_status.into(),
            }
            .emit(app);
        },
        TournamentLifecycle::LegacyUnknown
        | TournamentLifecycle::Preparing
        | TournamentLifecycle::Running => {
            return Err("supervisor attempted to finalize a non-terminal lifecycle".into());
        },
    }
    Ok(())
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
        limits: LaunchLimits::default(),
    })
}

/// Warm participants, create the durable row, and resolve one runner input.
/// The generation is recorded on the phase as soon as the row exists so a
/// supervisor can finalize it even if later preparation panics or fails.
async fn prepare_tournament(
    app: &tauri::AppHandle,
    params: LaunchParams,
    validated: ValidatedLaunch,
    phase: Arc<tokio::sync::Mutex<TournamentPhase>>,
    generation: u64,
    control: TournamentControl,
) -> Result<(Arc<Mutex<EvalStore>>, TournamentRun), String> {
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
    let game_config = validated.game_config;
    let target_games_per_matchup = validated.target_games_per_matchup;
    let tournament_seed = match validated.tournament_seed {
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
        validated.move_timeout_ms,
        validated.preprocessing_timeout_ms,
    );
    let orchestrator_config = tournament_config::orchestrator_config(
        validated.move_timeout_ms,
        validated.preprocessing_timeout_ms,
        validated.max_parallel,
    );

    let total = validated.total_games;
    let plan_summary = plan_summary(
        &params.target,
        canonical_players.len(),
        &params.factory,
        target_games_per_matchup,
        validated.move_timeout_ms,
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
            max_parallel: validated.max_parallel,
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
    let cancel = control.token();
    warmup_bots(app, &params.bots, &cancel, preflight).await?;
    if cancel.is_cancelled() {
        return Err("tournament start was cancelled".into());
    }

    let created =
        EvalSession::create_tournament(store.clone(), spec.clone(), canonical_players.clone())
            .await
            .map_err(|e| format!("create tournament: {e}"))?;
    let tournament_id = created.tournament_id;
    if !phase
        .lock()
        .await
        .record_tournament_id(generation, tournament_id.0)
    {
        return Err(format!(
            "tournament generation {generation} lost ownership before row {} was adopted",
            tournament_id.0
        ));
    }
    let adopted = store
        .lock()
        .mark_tournament_preparing(tournament_id)
        .map_err(|error| format!("mark tournament preparing: {error}"))?;
    if !adopted {
        return Err(format!(
            "tournament {} could not enter preparing state",
            tournament_id.0
        ));
    }

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

    Ok((store, run))
}

/// Smoke-build each distinct bot in both seat assignments before the
/// tournament starts.
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
    cancel: &tokio_util::sync::CancellationToken,
    preflight: PreflightConfig,
) -> Result<(), String> {
    let distinct = distinct_by_agent(bots);
    let seats = [PlayerSlot::Player1, PlayerSlot::Player2];
    let total = u32::try_from(distinct.len().saturating_mul(seats.len())).unwrap_or(u32::MAX);
    let mut done = 0;
    for bot in distinct {
        for seat in seats {
            let _ = TournamentPreparingEvent {
                done,
                total,
                current: Some(format!("{} ({seat:?})", bot.agent_id)),
            }
            .emit(app);
            let probe = preflight_bot_in_slot(
                bot.run_command.clone(),
                bot.working_dir.clone(),
                bot.agent_id.clone(),
                seat,
                preflight.clone(),
            );
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err("tournament start was cancelled".into()),
                result = probe => {
                    result.map_err(|e| {
                        format!(
                            "bot `{}` failed compatibility check as {seat:?}: {e}",
                            bot.agent_id
                        )
                    })?;
                }
            }
            done += 1;
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

/// Request the owning generation to stop and wait until its finalizer has
/// persisted the terminal outcome and released the slot.
#[tauri::command]
#[specta::specta]
pub async fn stop_tournament(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<TournamentRuntimeStatus, String> {
    request_stop_and_wait(&app, &state, TournamentStopCause::User, None).await?;
    Ok(current_tournament_runtime(&state).await)
}

/// Frontend-confirmed native close. Uses a distinct durable reason and a
/// bounded drain so unfinished work is never presented as resumable.
#[tauri::command]
#[specta::specta]
pub async fn shutdown_tournament(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<TournamentRuntimeStatus, String> {
    shutdown_tournament_for_app(&app, &state).await?;
    Ok(current_tournament_runtime(&state).await)
}

pub(crate) async fn shutdown_tournament_for_app(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<(), String> {
    request_stop_and_wait(
        app,
        state,
        TournamentStopCause::AppShutdown,
        Some(Duration::from_secs(7)),
    )
    .await
}

async fn request_stop_and_wait(
    app: &tauri::AppHandle,
    state: &AppState,
    cause: TournamentStopCause,
    timeout: Option<Duration>,
) -> Result<(), String> {
    let (lease, newly_stopping) = {
        let mut phase = state.tournament_phase.lock().await;
        let newly_stopping = !matches!(*phase, TournamentPhase::Stopping(_));
        (phase.request_stop(cause), newly_stopping)
    };
    let Some(mut lease) = lease else {
        return Ok(());
    };

    if newly_stopping {
        let _ = TournamentStoppingEvent {
            tournament_id: lease.tournament_id,
        }
        .emit(app);
    }

    let wait = async {
        loop {
            if let Some(done) = lease.completion.borrow().clone() {
                return done.result;
            }
            lease
                .completion
                .changed()
                .await
                .map_err(|_| "tournament supervisor closed without completion".to_string())?;
        }
    };
    match timeout {
        Some(limit) => tokio::time::timeout(limit, wait).await.map_err(|_| {
            format!(
                "tournament did not stop within the {} second shutdown limit",
                limit.as_secs()
            )
        })?,
        None => wait.await,
    }
}

/// Current app-owned runner phase. Durable lifecycle remains on tournament
/// snapshots; this survives navigation/reload and exposes Starting/Stopping.
#[tauri::command]
#[specta::specta]
pub async fn tournament_status(
    state: tauri::State<'_, AppState>,
) -> Result<TournamentRuntimeStatus, String> {
    Ok(current_tournament_runtime(&state).await)
}

/// List tournaments in the store, newest first, projected from durable
/// lifecycle plus the shared execution counters.
#[tauri::command]
#[specta::specta]
pub async fn list_tournaments(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<TournamentSummary>, String> {
    let running_id = current_tournament_runtime(&state).await.tournament_id;
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
        let max_failures = stored_tournament_params(&rec)?.max_failures_per_pair;
        let total = planned_slots(&rec.format, players.len(), target)?;
        let progress = execution_progress(&st, total, max_failures)?;
        validate_projection_truth(&rec, &progress)?;
        let terminal = terminal_projection(&rec)?;
        out.push(TournamentSummary {
            id: rec.id.0,
            name: rec.name,
            format: rec.format,
            created_at: rec.created_at,
            running: running_id == Some(rec.id.0),
            lifecycle: rec.lifecycle.into(),
            terminal,
            rating_readiness: rec.rating_status.into(),
            rating_reason: rec.rating_reason,
            progress,
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
    let max_failures = stored_tournament_params(&rec)?.max_failures_per_pair;
    let total = planned_slots(&rec.format, players.len(), target_games)?;
    let progress = execution_progress(&st, total, max_failures)?;
    validate_projection_truth(&rec, &progress)?;
    let terminal = terminal_projection(&rec)?;

    Ok(StandingsSnapshot {
        tournament_id,
        name: rec.name,
        format: rec.format,
        anchor_id,
        lifecycle: rec.lifecycle.into(),
        terminal,
        rating_readiness: rec.rating_status.into(),
        rating_reason: rec.rating_reason,
        progress,
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
    let running = current_tournament_runtime(&state).await.tournament_id == Some(tournament_id);
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

    let params = stored_tournament_params(&rec)?;
    let games_per_matchup = rec.target_games_per_matchup.unwrap_or(0);
    let total = planned_slots(&rec.format, players.len(), games_per_matchup)?;
    let progress = execution_progress(&tournament_state, total, params.max_failures_per_pair)?;
    validate_projection_truth(&rec, &progress)?;
    let terminal = terminal_projection(&rec)?;
    let exhausted_failures = exhausted_failure_ids(&attempts, params.max_failures_per_pair)?;

    let mut games = Vec::with_capacity(progress.successful_games as usize);
    let mut failures = Vec::with_capacity(progress.failed_attempts as usize);
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
            AttemptOutcome::Failure {
                failure_reason,
                report,
                ..
            } => {
                let (kind, timeout_phase, failing_player_id, reason) = match report {
                    Some(report) => typed_failure_projection(report),
                    None => {
                        let (kind, phase, player) = stored_failure_projection(
                            failure_reason,
                            attempt.key.orientation,
                            &attempt.key.player1_id,
                            &attempt.key.player2_id,
                        );
                        (kind, phase, player, failure_reason.clone())
                    },
                };
                failures.push(StoredMatchFailure {
                    attempt_id: attempt.id,
                    match_id: attempt.key.match_id,
                    player1_id: attempt.key.player1_id.clone(),
                    player2_id: attempt.key.player2_id.clone(),
                    repetition_index: attempt.key.repetition_index,
                    attempt_index: attempt.key.attempt_index,
                    rat_id,
                    failing_player_id,
                    kind,
                    timeout_phase,
                    reason,
                    exhausted: exhausted_failures.contains(&attempt.id),
                });
            },
        }
    }
    let slots = stored_slot_projection(
        &rec,
        &players,
        &attempts,
        games_per_matchup,
        params.max_failures_per_pair,
        params.seat_policy,
    )?;

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
        lifecycle: rec.lifecycle.into(),
        terminal,
        rating_readiness: rec.rating_status.into(),
        rating_reason: rec.rating_reason,
        paired: matches!(params.seat_policy, SeatPolicy::Paired),
        methodology,
        created_at: rec.created_at,
        started_at: rec.started_at,
        terminal_at: rec.terminal_at,
        last_finished_at,
        plan_summary,
        players,
        games_per_matchup,
        progress,
        standings,
        games,
        failures,
        slots,
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

fn typed_failure_projection(
    report: &AttemptFailureReport,
) -> (FailureKind, Option<TimeoutPhase>, Option<String>, String) {
    (
        report.kind.into(),
        report.phase.map(TimeoutPhase::from),
        report.failing_player_id.clone(),
        report.message.clone(),
    )
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

fn planned_slots(format: &str, players: usize, target: u32) -> Result<u32, String> {
    let players = u32::try_from(players).map_err(|_| "participant count exceeds u32")?;
    let matchups = match format {
        "gauntlet" => players.saturating_sub(1),
        _ => players
            .checked_mul(players.saturating_sub(1))
            .and_then(|value| value.checked_div(2))
            .ok_or_else(|| "tournament matchup count overflow".to_string())?,
    };
    matchups
        .checked_mul(target)
        .ok_or_else(|| "tournament planned-slot count overflow".to_string())
}

fn execution_progress(
    state: &TournamentState,
    planned_slots: u32,
    max_failures: u32,
) -> Result<TournamentProgress, String> {
    state
        .execution_summary(planned_slots, max_failures)
        .map(TournamentProgress::from)
        .map_err(|error| format!("tournament progress: {error}"))
}

fn stored_tournament_params(rec: &TournamentRecord) -> Result<TournamentParams, String> {
    match TournamentParams::from_json(&rec.params_json) {
        Ok(params) => Ok(params),
        Err(_) if rec.lifecycle == TournamentLifecycle::LegacyUnknown => Ok(TournamentParams {
            max_failures_per_pair: 0,
            seat_policy: SeatPolicy::Legacy,
        }),
        Err(error) => Err(format!(
            "tournament {} has invalid durable planner parameters: {error}",
            rec.id.0
        )),
    }
}

fn validate_projection_truth(
    rec: &TournamentRecord,
    progress: &TournamentProgress,
) -> Result<(), String> {
    if rec.lifecycle == TournamentLifecycle::Completed
        && progress.terminal_slots != progress.planned_slots
    {
        return Err(format!(
            "completed tournament {} has {}/{} terminal schedule slots",
            rec.id.0, progress.terminal_slots, progress.planned_slots
        ));
    }
    match rec.terminal.as_ref().map(|terminal| terminal.kind) {
        Some(TournamentTerminalKind::Completed) if progress.exhausted_slots > 0 => Err(format!(
            "tournament {} is marked completed but has {} exhausted slots",
            rec.id.0, progress.exhausted_slots
        )),
        Some(TournamentTerminalKind::CompletedWithFailures) if progress.exhausted_slots == 0 => {
            Err(format!(
                "tournament {} is marked completed-with-failures but has no exhausted slots",
                rec.id.0
            ))
        },
        _ => Ok(()),
    }
}

fn terminal_projection(rec: &TournamentRecord) -> Result<Option<TournamentTerminalState>, String> {
    match (&rec.terminal, &rec.terminal_at) {
        (None, None) => Ok(None),
        (Some(terminal), Some(at)) => Ok(Some(TournamentTerminalState::from_store(
            terminal,
            at.clone(),
        ))),
        _ => Err(format!(
            "tournament {} has a partial terminal record",
            rec.id.0
        )),
    }
}

fn exhausted_failure_ids(
    attempts: &[AttemptRecord],
    max_failures: u32,
) -> Result<HashSet<i64>, String> {
    if max_failures == 0 {
        return Ok(HashSet::new());
    }
    let mut by_slot: HashMap<MatchupKey, Vec<(u32, i64)>> = HashMap::new();
    for attempt in attempts {
        if matches!(attempt.outcome, AttemptOutcome::Failure { .. }) {
            let key = MatchupKey::from_pair(
                &attempt.key.player1_id,
                &attempt.key.player2_id,
                &attempt.key.game_config_id,
                attempt.key.repetition_index,
            );
            by_slot
                .entry(key)
                .or_default()
                .push((attempt.key.attempt_index, attempt.id));
        }
    }
    let max_failures = usize::try_from(max_failures)
        .map_err(|_| "failure budget exceeds platform usize".to_string())?;
    Ok(by_slot
        .into_values()
        .filter_map(|mut attempts| {
            attempts.sort_by_key(|(attempt_index, _)| *attempt_index);
            attempts
                .get(max_failures.saturating_sub(1))
                .map(|(_, id)| *id)
        })
        .collect())
}

fn stored_slot_projection(
    rec: &TournamentRecord,
    players: &[String],
    attempts: &[AttemptRecord],
    target: u32,
    max_failures: u32,
    seat_policy: SeatPolicy,
) -> Result<Vec<StoredTournamentSlot>, String> {
    let capacity = usize::try_from(planned_slots(&rec.format, players.len(), target)?)
        .map_err(|_| "planned slots exceed platform usize".to_string())?;
    let mut slots = Vec::with_capacity(capacity);
    for (a, b) in expected_pairs(&rec.format, players)? {
        for repetition_index in 0..target {
            let key = MatchupKey::from_pair(&a, &b, &rec.game_config_id, repetition_index);
            let mut slot_attempts: Vec<&AttemptRecord> = attempts
                .iter()
                .filter(|attempt| {
                    MatchupKey::from_pair(
                        &attempt.key.player1_id,
                        &attempt.key.player2_id,
                        &attempt.key.game_config_id,
                        attempt.key.repetition_index,
                    ) == key
                })
                .collect();
            slot_attempts.sort_by_key(|attempt| attempt.key.attempt_index);
            let state = if let Some(success) = slot_attempts
                .iter()
                .find(|attempt| matches!(attempt.outcome, AttemptOutcome::Success { .. }))
            {
                StoredSlotState::Successful {
                    attempt_id: success.id,
                    match_id: success.key.match_id,
                }
            } else {
                let failures: Vec<_> = slot_attempts
                    .iter()
                    .filter(|attempt| matches!(attempt.outcome, AttemptOutcome::Failure { .. }))
                    .collect();
                let failure_count = u32::try_from(failures.len())
                    .map_err(|_| "slot failure count exceeds u32".to_string())?;
                if max_failures > 0 && failure_count >= max_failures {
                    let exhaustion_index = usize::try_from(max_failures - 1)
                        .map_err(|_| "failure budget exceeds platform usize".to_string())?;
                    StoredSlotState::Exhausted {
                        last_failure_attempt_id: failures[exhaustion_index].id,
                        failed_attempts: failure_count,
                    }
                } else if rec.lifecycle == TournamentLifecycle::LegacyUnknown {
                    StoredSlotState::LegacyUnknown
                } else {
                    StoredSlotState::Pending
                }
            };
            let orientation = match seat_policy {
                SeatPolicy::Paired if !repetition_index.is_multiple_of(2) => {
                    SeatOrientation::Flipped
                },
                SeatPolicy::Legacy | SeatPolicy::Paired => SeatOrientation::Canonical,
            };
            slots.push(StoredTournamentSlot {
                player1_id: a.clone(),
                player2_id: b.clone(),
                repetition_index,
                rat_id: stored_rat_id(orientation, &a, &b),
                state,
            });
        }
    }
    Ok(slots)
}

fn expected_pairs(format: &str, players: &[String]) -> Result<Vec<(String, String)>, String> {
    match format {
        "gauntlet" => {
            // Slot 0 is the challenger; it plays each opponent.
            let Some((challenger, opponents)) = players.split_first() else {
                return Err("stored gauntlet has no challenger".to_string());
            };
            Ok(opponents
                .iter()
                .map(|opp| (challenger.clone(), opp.clone()))
                .collect())
        },
        _ => {
            let mut pairs = Vec::new();
            for i in 0..players.len() {
                for j in (i + 1)..players.len() {
                    pairs.push((players[i].clone(), players[j].clone()));
                }
            }
            Ok(pairs)
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

    fn test_lease(
        generation: u64,
        tournament_id: Option<i64>,
    ) -> (
        TournamentLease,
        tokio::sync::watch::Sender<Option<TournamentSupervisorCompletion>>,
    ) {
        let (completion_tx, completion) = tokio::sync::watch::channel(None);
        (
            TournamentLease {
                generation,
                tournament_id,
                control: TournamentControl::new(),
                completion,
            },
            completion_tx,
        )
    }

    #[test]
    fn owned_slot_is_blocked_until_its_generation_clears() {
        assert!(reservation_check(&TournamentPhase::Idle).is_ok());
        let (lease, _completion) = test_lease(7, None);
        assert!(reservation_check(&TournamentPhase::Starting(lease.clone())).is_err());
        assert!(reservation_check(&TournamentPhase::Running(lease.with_tournament_id(1))).is_err());

        let mut stopping = TournamentPhase::Stopping(lease.with_tournament_id(1));
        assert!(reservation_check(&stopping).is_err());
        assert!(!stopping.clear_if_generation(6));
        assert!(reservation_check(&stopping).is_err());
        assert!(stopping.clear_if_generation(7));
        assert!(reservation_check(&stopping).is_ok());
    }

    #[test]
    fn stop_is_idempotent_and_preserves_generation_identity() {
        let (lease, _completion) = test_lease(11, Some(42));
        let mut phase = TournamentPhase::Running(lease);
        let first = phase
            .request_stop(TournamentStopCause::User)
            .expect("running lease");
        let second = phase
            .request_stop(TournamentStopCause::AppShutdown)
            .expect("stopping lease");
        assert_eq!(first.generation, 11);
        assert_eq!(second.generation, 11);
        assert_eq!(second.tournament_id, Some(42));
        assert_eq!(
            second.control.stop_cause(),
            Some(TournamentStopCause::AppShutdown)
        );
        assert!(matches!(phase, TournamentPhase::Stopping(_)));
    }

    #[test]
    fn stop_a_must_finish_before_b_can_own_the_slot() {
        let (lease_a, _completion_a) = test_lease(20, None);
        let mut phase = TournamentPhase::Starting(lease_a);

        let stopped_a = phase
            .request_stop(TournamentStopCause::User)
            .expect("starting generation A");
        assert_eq!(stopped_a.generation, 20);
        assert!(reservation_check(&phase).is_err());

        // A may still create/adopt its row while the stop request races with
        // warmup completion. It stays Stopping and cannot be promoted back to
        // Running after cancellation has won.
        assert!(phase.record_tournament_id(20, 100));
        assert!(!phase.promote_running(20));
        assert!(matches!(phase, TournamentPhase::Stopping(_)));

        assert!(phase.clear_if_generation(20));
        let (lease_b, _completion_b) = test_lease(21, None);
        phase = TournamentPhase::Starting(lease_b);

        // A's late supervisor cleanup cannot clear or promote B's generation.
        assert!(!phase.clear_if_generation(20));
        assert!(!phase.promote_running(20));
        assert_eq!(phase.lease().map(|lease| lease.generation), Some(21));
        assert!(reservation_check(&phase).is_err());
    }

    /// 7×7, no walls/mud, random symmetric — the interesting validation case.
    fn random_symmetric_factory() -> GameFactoryConfig {
        GameFactoryConfig {
            width: 7.0,
            height: 7.0,
            max_turns: 100.0,
            wall_density: 0.0,
            mud_density: 0.0,
            mud_range: 2.0,
            connected: true,
            symmetric: true,
            player_start: PlayerStart::Random,
            cheese_count: 4.0,
            cheese_symmetric: true,
        }
    }

    fn valid_launch() -> LaunchParams {
        let working_dir = env!("CARGO_MANIFEST_DIR").to_string();
        LaunchParams {
            bots: vec![pick("a", &working_dir), pick("b", &working_dir)],
            target: None,
            name: None,
            factory: random_symmetric_factory(),
            mazes_per_matchup: 1.0,
            move_timeout_ms: 200.0,
            preprocessing_timeout_ms: 2_000.0,
            max_parallel: 2.0,
            tournament_seed: Some(42.0),
        }
    }

    fn assert_validation_field(params: &LaunchParams, field: &str) {
        let Err(errors) = validate_launch_params(params) else {
            panic!("launch should be rejected");
        };
        assert!(
            errors.iter().any(|error| error.field == field),
            "missing field {field}; got {errors:?}"
        );
    }

    #[test]
    fn random_symmetric_odd_cheese_rejected() {
        let f = GameFactoryConfig {
            cheese_count: 5.0,
            ..random_symmetric_factory()
        };
        let Err(err) = f.to_game_config() else {
            panic!("odd symmetric cheese with random starts should be rejected");
        };
        assert!(err.to_string().contains("even cheese count"), "{err}");
    }

    #[test]
    fn random_symmetric_even_cheese_over_capacity_rejected() {
        // 7×7 odd board → cap = 49 - 5 = 44; 46 is over.
        let f = GameFactoryConfig {
            cheese_count: 46.0,
            ..random_symmetric_factory()
        };
        let Err(err) = f.to_game_config() else {
            panic!("over-capacity random symmetric cheese should be rejected");
        };
        assert!(err.to_string().contains("over capacity"), "{err}");
    }

    #[test]
    fn random_symmetric_even_within_capacity_builds_every_seed() {
        // The capacity cap exists so no *seed* can fail mid-tournament: sweep.
        let f = GameFactoryConfig {
            cheese_count: 10.0,
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
        for (w, h) in [
            (1.0, 7.0),
            (65.0, 7.0),
            (300.0, 7.0),
            (7.0, 1.0),
            (7.0, 65.0),
        ] {
            let f = GameFactoryConfig {
                width: w,
                height: h,
                ..random_symmetric_factory()
            };
            assert!(f.to_game_config().is_err(), "{w}x{h} should be rejected");
        }
    }

    #[test]
    fn launch_validation_addresses_hostile_engine_and_methodology_fields() {
        let mut params = valid_launch();
        params.factory.width = 65.0;
        assert_validation_field(&params, "factory.width");

        let mut params = valid_launch();
        params.factory.width = 7.5;
        assert_validation_field(&params, "factory.width");

        let mut params = valid_launch();
        params.factory.mud_range = 16.0;
        assert_validation_field(&params, "factory.mud_range");

        let mut params = valid_launch();
        params.factory.max_turns = 1_024.0;
        assert_validation_field(&params, "factory.max_turns");

        let mut params = valid_launch();
        params.factory.cheese_count = 510.0;
        assert_validation_field(&params, "factory.cheese_count");

        let mut params = valid_launch();
        params.factory.wall_density = f64::NAN;
        assert_validation_field(&params, "factory.wall_density");

        let mut params = valid_launch();
        params.move_timeout_ms = 0.0;
        assert_validation_field(&params, "move_timeout_ms");

        let mut params = valid_launch();
        params.move_timeout_ms = f64::INFINITY;
        assert_validation_field(&params, "move_timeout_ms");

        let mut params = valid_launch();
        params.preprocessing_timeout_ms = f64::from(MAX_TIMEOUT_MS + 1);
        assert_validation_field(&params, "preprocessing_timeout_ms");

        let mut params = valid_launch();
        params.max_parallel = 0.0;
        assert_validation_field(&params, "max_parallel");

        let mut params = valid_launch();
        params.mazes_per_matchup = 1.5;
        assert_validation_field(&params, "mazes_per_matchup");

        let mut params = valid_launch();
        params.tournament_seed = Some((MAX_JS_SAFE_SEED + 1) as f64);
        assert_validation_field(&params, "tournament_seed");
    }

    #[test]
    fn launch_validation_rejects_invalid_bot_identity_command_and_directory() {
        let mut params = valid_launch();
        params.bots[1].agent_id = params.bots[0].agent_id.clone();
        params.bots[0].run_command = "  ".into();
        params.bots[0].working_dir = "/definitely/not/a/pyrat/directory".into();
        params.target = Some("missing".into());

        let Err(errors) = validate_launch_params(&params) else {
            panic!("launch should be rejected");
        };
        for field in [
            "bots.1.agent_id",
            "bots.0.run_command",
            "bots.0.working_dir",
            "target",
        ] {
            assert!(
                errors.iter().any(|error| error.field == field),
                "missing field {field}; got {errors:?}"
            );
        }
    }

    #[test]
    fn launch_validation_checks_total_game_arithmetic_and_cap() {
        let mut params = valid_launch();
        let working_dir = env!("CARGO_MANIFEST_DIR").to_string();
        params.bots.push(pick("c", &working_dir));
        params.mazes_per_matchup = 20_000.0;

        assert_validation_field(&params, "mazes_per_matchup");
    }

    #[test]
    fn valid_launch_resolves_checked_schedule() {
        let validated = validate_launch_params(&valid_launch()).expect("valid launch");
        assert_eq!(validated.target_games_per_matchup, 2);
        assert_eq!(validated.total_games, 2);
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
        let config_id = "cfg";
        let mut state = TournamentState::empty(TournamentId(1));
        for repetition_index in 0..15 {
            state.history.insert(
                MatchupKey::from_pair("alice", "bob", config_id, repetition_index),
                vec![pyrat_eval::MatchupAttempt {
                    attempt_index: 0,
                    outcome: pyrat_eval::MatchupOutcome::Success {
                        player1_score: 5.0,
                        player2_score: 3.0,
                    },
                }],
            );
        }
        state.history.insert(
            MatchupKey::from_pair("alice", "bob", config_id, 15),
            vec![pyrat_eval::MatchupAttempt {
                attempt_index: 0,
                outcome: pyrat_eval::MatchupOutcome::Failure,
            }],
        );

        // Fifteen scored games plus one exhausted slot is a completed
        // sixteen-slot schedule, not a permanently "15/16 running" run.
        let completed = state.execution_summary(16, 1).unwrap();
        assert_eq!(completed.successful_games, 15);
        assert_eq!(completed.terminal_slots, 16);
        assert_eq!(completed.exhausted_slots, 1);

        let retryable = state.execution_summary(16, 2).unwrap();
        assert_eq!(retryable.successful_games, 15);
        assert_eq!(retryable.terminal_slots, 15);
        assert_eq!(retryable.exhausted_slots, 0);
    }

    #[test]
    fn stored_paired_slots_keep_success_and_exhaustion_distinct() {
        use pyrat_eval_store::{
            AttemptFailureKind, AttemptKey, TournamentRatingStatus, TournamentTerminal,
            TournamentTerminalKind,
        };

        let record = TournamentRecord {
            id: TournamentId(1),
            name: Some("paired truth".into()),
            format: "round_robin".into(),
            target_games_per_matchup: Some(2),
            params_json: "{}".into(),
            game_config_id: "cfg".into(),
            tournament_seed: 42,
            methodology: None,
            lifecycle: TournamentLifecycle::Completed,
            started_at: Some("2026-08-12 10:00:00".into()),
            terminal_at: Some("2026-08-12 10:01:00".into()),
            terminal: Some(TournamentTerminal {
                kind: TournamentTerminalKind::CompletedWithFailures,
                reason: Some("one schedule slot exhausted its retry budget".into()),
            }),
            rating_status: TournamentRatingStatus::InsufficientGames,
            rating_reason: Some("each player needs four successful games".into()),
            created_at: "2026-08-12 09:59:00".into(),
        };
        let key = |repetition_index, orientation| AttemptKey {
            tournament_id: TournamentId(1),
            game_config_id: "cfg".into(),
            player1_id: "alice".into(),
            player2_id: "bob".into(),
            match_id: Some(u64::from(repetition_index) + 10),
            seed: 7,
            repetition_index,
            attempt_index: 0,
            orientation,
        };
        let attempts = vec![
            AttemptRecord {
                id: 1,
                key: key(0, SeatOrientation::Canonical),
                finished_at: "2026-08-12 10:00:30".into(),
                outcome: AttemptOutcome::Success {
                    player1_score: 5.0,
                    player2_score: 3.0,
                    turns: 20,
                    started_at: "2026-08-12 10:00:01".into(),
                },
            },
            AttemptRecord {
                id: 2,
                key: key(1, SeatOrientation::Flipped),
                finished_at: "2026-08-12 10:01:00".into(),
                outcome: AttemptOutcome::Failure {
                    failure_reason: "timeout: move: Player1".into(),
                    report: Some(AttemptFailureReport {
                        kind: AttemptFailureKind::Timeout,
                        phase: Some(pyrat_eval_store::AttemptFailurePhase::Move),
                        failing_player_id: Some("bob".into()),
                        message: "timeout: move: Player1".into(),
                    }),
                    started_at: Some("2026-08-12 10:00:31".into()),
                },
            },
        ];

        assert!(validate_projection_truth(
            &record,
            &TournamentProgress {
                planned_slots: 2,
                terminal_slots: 2,
                successful_games: 1,
                exhausted_slots: 1,
                failed_attempts: 1,
                running_matches: 0,
            },
        )
        .is_ok());
        assert!(validate_projection_truth(
            &record,
            &TournamentProgress {
                planned_slots: 2,
                terminal_slots: 1,
                successful_games: 1,
                exhausted_slots: 0,
                failed_attempts: 0,
                running_matches: 0,
            },
        )
        .is_err());

        let slots = stored_slot_projection(
            &record,
            &["alice".into(), "bob".into()],
            &attempts,
            2,
            1,
            SeatPolicy::Paired,
        )
        .unwrap();
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].rat_id, "alice");
        assert!(matches!(
            slots[0].state,
            StoredSlotState::Successful {
                attempt_id: 1,
                match_id: Some(10)
            }
        ));
        assert_eq!(slots[1].rat_id, "bob");
        assert!(matches!(
            slots[1].state,
            StoredSlotState::Exhausted {
                last_failure_attempt_id: 2,
                failed_attempts: 1
            }
        ));
    }
}
