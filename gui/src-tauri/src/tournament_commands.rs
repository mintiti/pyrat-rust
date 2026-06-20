//! Tauri commands for the tournament module.
//!
//! `start_tournament` creates the row (returning its id) and spawns the
//! runner on a background task. The store is the same SQLite the CLI uses, at
//! the app-data path (`tournament_paths`). Read commands open a fresh
//! connection each call — WAL allows concurrent readers, and they're
//! infrequent.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use pyrat::game::builder::{
    CheeseStrategy, GameBuilder, GameConfig, MazeParams, MazeStrategy, PlayerStrategy,
};
use pyrat_eval::{
    EvalSession, MatchupKey, ResolvedPlayer, TournamentParams, TournamentSpec, TournamentState,
};
use pyrat_eval_store::{EvalStore, TournamentId};
use pyrat_orchestrator::{PlayerSpec, ReplayEvent, ReplayFile};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::commands::{MazeState, MudEntry, PlayerState, WallEntry};
use crate::state::{AppState, TournamentPhase};
use crate::tournament_config;
use crate::tournament_events::StandingRow;
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
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Type)]
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

/// Create a tournament and start running it in the background. Returns the
/// new tournament id. Rejects if one is already running.
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
    {
        let mut phase = state.tournament_phase.lock().await;
        match *phase {
            TournamentPhase::Idle => *phase = TournamentPhase::Starting,
            TournamentPhase::Starting => return Err("a tournament is already starting".into()),
            TournamentPhase::Running { .. } => return Err("a tournament is already running".into()),
        }
    }

    match build_and_spawn(&app, params).await {
        Ok((tournament_id, cancel, handle)) => {
            let mut phase = state.tournament_phase.lock().await;
            if matches!(*phase, TournamentPhase::Starting) {
                *phase = TournamentPhase::Running {
                    tournament_id,
                    cancel,
                    handle,
                };
                Ok(tournament_id)
            } else {
                // A `stop_tournament` landed during the launch window (it reset
                // the slot to Idle). Honor it: cancel the runner we just spawned
                // and drain it, rather than promoting a tournament the user
                // already asked to stop.
                cancel.cancel();
                let _ = handle.await;
                Err("tournament start was cancelled".into())
            }
        },
        Err(e) => {
            // Roll back the reservation so a failed launch can't wedge the slot.
            let mut phase = state.tournament_phase.lock().await;
            if matches!(*phase, TournamentPhase::Starting) {
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

/// Create the tournament row and spawn its runner. Returns the new id plus
/// the cancel/handle the caller installs into `TournamentPhase::Running`.
/// Phase management lives entirely in `start_tournament`; this is the fallible
/// build body it wraps so every error path releases the `Starting` slot.
async fn build_and_spawn(
    app: &tauri::AppHandle,
    params: LaunchParams,
) -> Result<(i64, CancellationToken, tokio::task::JoinHandle<()>), String> {
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
    game_config
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
        game_config: game_config.clone(),
        tournament_seed,
    };

    let created =
        EvalSession::create_tournament(store.clone(), spec.clone(), canonical_players.clone())
            .await
            .map_err(|e| format!("create tournament: {e}"))?;
    let tournament_id = created.tournament_id;

    let replay_dir = crate::tournament_paths::tournament_replay_dir(app, tournament_id.0)?;
    let cancel = CancellationToken::new();
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
    let cancel_for_task = cancel.clone();
    let handle = tokio::spawn(async move {
        if let Err(e) = run_tournament(app_for_task, store, run, cancel_for_task).await {
            warn!(error = %e, "tournament runner exited with error");
        }
    });

    Ok((tournament_id.0, cancel, handle))
}

/// Request the running tournament to stop and wait for it to drain. The runner
/// shuts the session down gracefully and emits `TournamentAbortedEvent`. No-op
/// if nothing is running.
#[tauri::command]
#[specta::specta]
pub async fn stop_tournament(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let running = {
        let mut phase = state.tournament_phase.lock().await;
        std::mem::replace(&mut *phase, TournamentPhase::Idle)
    };
    if let TournamentPhase::Running { cancel, handle, .. } = running {
        cancel.cancel();
        let _ = handle.await;
    }
    Ok(())
}

/// The currently-running tournament id, if any. The frontend reads this on
/// load / tab switch to restore the live chip after a navigation.
#[tauri::command]
#[specta::specta]
pub async fn tournament_status(state: tauri::State<'_, AppState>) -> Result<Option<i64>, String> {
    let phase = state.tournament_phase.lock().await;
    Ok(match &*phase {
        TournamentPhase::Running { tournament_id, .. } => Some(*tournament_id),
        // Starting: the row isn't created yet, so there's no id to report —
        // the chip appears once the runner is promoted to Running.
        TournamentPhase::Starting | TournamentPhase::Idle => None,
    })
}

/// List tournaments in the store, newest first, with finished-inference.
#[tauri::command]
#[specta::specta]
pub async fn list_tournaments(app: tauri::AppHandle) -> Result<Vec<TournamentSummary>, String> {
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
}
