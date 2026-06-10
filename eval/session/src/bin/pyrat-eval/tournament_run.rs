//! Tournament run executor. Takes a `ResolvedRun`, wires the store +
//! planner + sinks, and drives the session to completion.
//!
//! Two paths:
//! - **New**: bootstrap the tournament via `EvalSession::create_tournament`,
//!   get the `(tournament_id, game_config_id)` pair, build the planner,
//!   start.
//! - **Resume (`--resume <id>`)**: query the store for the existing
//!   tournament, reuse its `game_config_id` and `tournament_seed`, build
//!   the planner matching the stored spec, start.
//!
//! Seed handling, gauntlet player ordering, pre-spawn validation, and
//! state capture follow the contract pinned in the source plan.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use pyrat_eval::MatchupOutcome;
use pyrat_eval::{
    EvalMatchDescriptor, EvalSession, GauntletPlanner, GauntletPlannerConfig, Planner,
    ResolvedPlayer, RoundRobinPlanner, RoundRobinPlannerConfig, SessionConfig, SessionMode,
    TournamentParams, TournamentSpec, TournamentState,
};
use pyrat_eval_store::{EloOptions, EvalStore, TournamentId};
use pyrat_host::wire::TimingMode;
use pyrat_orchestrator::{DirectoryWriter, MatchSink, ReplaySink, SinkRole, Timing};
use serde::Serialize;

use crate::game_config_build::{build_game_config, ResolvedGameChoice};
use crate::orchestrator_config_build::build_orchestrator_config;
use crate::tournament_config::{
    EloSection, GameSection, GauntletSection, PlayerEntry, TimingSection, TournamentConfig,
};
use crate::tournament_resolve::{FormatChoice, ResolvedRun, SeedSource};

/// Execute the tournament described by `resolved`. Returns the final
/// `TournamentState` for the caller to render (standings, JSON).
///
/// If `resolved.save_as` is set, materializes the spec to TOML
/// **before** the tournament runs so the user gets the spec file even
/// if the run aborts.
pub async fn run_tournament_main(
    resolved: ResolvedRun,
) -> Result<TournamentState, Box<dyn std::error::Error>> {
    if let Some(save_path) = resolved.save_as.as_ref() {
        write_save_as(&resolved, save_path)?;
    }

    let game_config = build_game_config(&resolved.game)?;
    let store = Arc::new(Mutex::new(EvalStore::open(&resolved.store_path)?));

    // Realize the seed and the (tournament_id, game_config_id) pair. On
    // resume, the store is the source of truth; on a new tournament,
    // we validate the game config *before* bootstrap so an invalid
    // config (e.g. too much cheese for the board) doesn't leave a
    // dangling tournament row behind.
    let (tournament_id, game_config_id, tournament_seed) = match resolved.resume {
        Some(id) => {
            let (id, gc_id, seed) = realize_resume(&store, id, &resolved.seed)?;
            validate_game_config_with_seed(&game_config, seed)?;
            (id, gc_id, seed)
        },
        None => {
            let seed = match resolved.seed {
                SeedSource::Explicit(s) | SeedSource::Generated(s) => s,
                SeedSource::FromStoreOnResume => {
                    return Err("internal: FromStoreOnResume on non-resume path".into())
                },
            };
            validate_game_config_with_seed(&game_config, seed)?;
            bootstrap_new(&store, &resolved, &game_config, seed).await?
        },
    };

    let orch_config = build_orchestrator_config(&resolved.timing, resolved.max_parallel);
    let per_match_timing = Timing {
        mode: TimingMode::Wait,
        move_timeout_ms: resolved.timing.move_timeout_ms,
        preprocessing_timeout_ms: resolved.timing.preprocessing_timeout_ms,
    };

    // Extra sinks: optional ReplaySink in the configured directory.
    let mut extra_sinks: Vec<(SinkRole, Arc<dyn MatchSink<EvalMatchDescriptor>>)> = Vec::new();
    if let Some(dir) = resolved.replay_dir.as_ref() {
        let writer = Arc::new(
            DirectoryWriter::new(dir.clone())
                .map_err(|e| format!("--replay-dir: failed to open {}: {e}", dir.display()))?,
        );
        let replay: Arc<dyn MatchSink<EvalMatchDescriptor>> = Arc::new(
            ReplaySink::new(writer)
                .with_engine_version(format!("pyrat-eval/{}", env!("CARGO_PKG_VERSION"))),
        );
        extra_sinks.push((SinkRole::Optional, replay));
    }

    // The format only decides which planner gets built; the start/await
    // path is shared via `Box<dyn Planner>`.
    let planner: Box<dyn Planner> = match &resolved.format {
        FormatChoice::RoundRobin => Box::new(RoundRobinPlanner::new(RoundRobinPlannerConfig {
            players: resolved.players.clone(),
            game_config: game_config.clone(),
            game_config_id,
            timing: per_match_timing,
            tournament_id,
            target_per_pair: resolved.target_games_per_matchup,
            max_failures_per_pair: resolved.max_failures_per_pair,
            tournament_seed,
        })),
        FormatChoice::Gauntlet {
            challenger,
            opponents,
        } => {
            let (challenger_p, opponent_ps) =
                split_gauntlet_players(&resolved.players, challenger, opponents)?;
            Box::new(GauntletPlanner::new(GauntletPlannerConfig {
                challenger: challenger_p,
                opponents: opponent_ps,
                game_config: game_config.clone(),
                game_config_id,
                timing: per_match_timing,
                tournament_id,
                target_each: resolved.target_games_per_matchup,
                max_failures_per_pair: resolved.max_failures_per_pair,
                tournament_seed,
            }))
        },
    };
    let session = EvalSession::start_with_extra_sinks(
        store.clone(),
        SessionMode { tournament_id },
        planner,
        orch_config,
        build_elo_options(&resolved),
        SessionConfig::default(),
        extra_sinks,
    )
    .await?;
    let final_state = await_session(session).await?;

    render_standings(&final_state, resolved.results_json.as_deref())?;
    Ok(final_state)
}

fn build_elo_options(resolved: &ResolvedRun) -> EloOptions {
    EloOptions::new(resolved.anchor.clone()).anchor_elo(resolved.anchor_elo)
}

/// Fail before bots launch if the game config refuses the resolved seed
/// (e.g. too many cheese for the board). Runs on both the new-tournament
/// path (before `bootstrap_new` so an invalid config doesn't leave a
/// dangling row) and the resume path.
fn validate_game_config_with_seed(
    game_config: &pyrat::game::builder::GameConfig,
    seed: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    game_config
        .clone()
        .create(Some(seed))
        .map_err(|e| format!("invalid game config: {e}"))?;
    Ok(())
}

/// Capture state BEFORE consuming the session via `join`. The state
/// watch is the only handle on the final standings; reading it after
/// `join` would race with the session's drop chain.
async fn await_session(
    session: EvalSession,
) -> Result<TournamentState, Box<dyn std::error::Error>> {
    let state_rx = session.state();
    session.join().await?;
    let final_state = state_rx.borrow().clone();
    Ok(final_state)
}

/// Bootstrap a fresh tournament and return its identity. The
/// `[challenger, ...opponents]` ordering for gauntlet is honored here
/// because the planner's `expected_players()` returns that order and
/// resume validation compares slot-to-slot.
async fn bootstrap_new(
    store: &Arc<Mutex<EvalStore>>,
    resolved: &ResolvedRun,
    game_config: &pyrat::game::builder::GameConfig,
    seed: u64,
) -> Result<(TournamentId, String, u64), Box<dyn std::error::Error>> {
    let (format_str, canonical_players) = match &resolved.format {
        FormatChoice::RoundRobin => ("round_robin".to_string(), resolved.players.clone()),
        FormatChoice::Gauntlet {
            challenger,
            opponents,
        } => {
            let (c, mut ops) = split_gauntlet_players(&resolved.players, challenger, opponents)?;
            let mut v = Vec::with_capacity(1 + ops.len());
            v.push(c);
            v.append(&mut ops);
            ("gauntlet".to_string(), v)
        },
    };
    let params = TournamentParams {
        max_failures_per_pair: resolved.max_failures_per_pair,
    };
    let spec = TournamentSpec {
        format: format_str,
        target_games_per_matchup: Some(resolved.target_games_per_matchup),
        params_json: params.to_json(),
        game_config: game_config.clone(),
        tournament_seed: seed,
    };
    let created = EvalSession::create_tournament(store.clone(), spec, canonical_players).await?;
    Ok((created.tournament_id, created.game_config_id, seed))
}

/// On resume, the store carries the seed and game_config_id. Validate
/// explicit-seed mismatches before the bots launch so users get a clear
/// error rather than a cryptic `TournamentMismatch` from the planner
/// guard.
fn realize_resume(
    store: &Arc<Mutex<EvalStore>>,
    id: TournamentId,
    seed_source: &SeedSource,
) -> Result<(TournamentId, String, u64), Box<dyn std::error::Error>> {
    let stored = {
        let store = store.lock();
        store
            .get_tournament(id)?
            .ok_or_else(|| format!("tournament {} not found in store", id.0))?
    };
    let seed = match seed_source {
        SeedSource::Explicit(s) => {
            if *s != stored.tournament_seed {
                return Err(format!(
                    "seed mismatch on resume: --seed {} does not match stored {} (tournament {})",
                    s, stored.tournament_seed, id.0
                )
                .into());
            }
            *s
        },
        SeedSource::FromStoreOnResume => stored.tournament_seed,
        SeedSource::Generated(_) => return Err("internal: Generated seed on resume path".into()),
    };
    Ok((id, stored.game_config_id, seed))
}

// ── --save-as materializer ───────────────────────────────────────────

fn write_save_as(
    resolved: &ResolvedRun,
    save_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let save_dir = save_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(save_dir).map_err(|e| {
        format!(
            "--save-as: failed to create parent directory {}: {e}",
            save_dir.display()
        )
    })?;
    let cfg = to_saveable_config(resolved, save_dir);
    let toml_text = toml::to_string_pretty(&cfg)?;
    fs::write(save_path, toml_text)
        .map_err(|e| format!("--save-as: failed to write {}: {e}", save_path.display()))?;
    Ok(())
}

/// Project a `ResolvedRun` back into the `TournamentConfig` shape that
/// `--config` deserializes. Paths get rebased to be relative to
/// `save_dir` if possible, otherwise written absolute. Implicit and
/// store-on-resume seeds are intentionally omitted (a saved blueprint
/// stays decoupled from any one instance's seed).
fn to_saveable_config(resolved: &ResolvedRun, save_dir: &Path) -> TournamentConfig {
    let format = match &resolved.format {
        FormatChoice::RoundRobin => "round_robin",
        FormatChoice::Gauntlet { .. } => "gauntlet",
    };
    let game = Some(game_section_from(&resolved.game));
    let timing = Some(TimingSection {
        move_timeout_ms: Some(resolved.timing.move_timeout_ms),
        preprocessing_timeout_ms: Some(resolved.timing.preprocessing_timeout_ms),
        startup_timeout_ms: Some(resolved.timing.startup_timeout_ms),
        configure_timeout_ms: Some(resolved.timing.configure_timeout_ms),
        network_grace_ms: Some(resolved.timing.network_grace_ms),
    });
    let elo = Some(EloSection {
        anchor: Some(resolved.anchor.clone()),
        anchor_elo: Some(resolved.anchor_elo),
    });
    let players = resolved
        .players
        .iter()
        .map(|p| player_entry_from(p, save_dir))
        .collect();
    let gauntlet = match &resolved.format {
        FormatChoice::Gauntlet {
            challenger,
            opponents,
        } => Some(GauntletSection {
            challenger: challenger.clone(),
            opponents: opponents.clone(),
        }),
        FormatChoice::RoundRobin => None,
    };
    let seed = match resolved.seed {
        SeedSource::Explicit(s) => Some(s),
        SeedSource::Generated(_) | SeedSource::FromStoreOnResume => None,
    };

    TournamentConfig {
        store_path: Some(make_relative_or_absolute(&resolved.store_path, save_dir)),
        replay_dir: resolved
            .replay_dir
            .as_ref()
            .map(|p| make_relative_or_absolute(p, save_dir)),
        seed,
        format: Some(format.into()),
        target_games_per_matchup: Some(resolved.target_games_per_matchup),
        max_failures_per_pair: Some(resolved.max_failures_per_pair),
        max_parallel: Some(resolved.max_parallel),
        game,
        timing,
        elo,
        players,
        gauntlet,
    }
}

fn game_section_from(choice: &ResolvedGameChoice) -> GameSection {
    match choice {
        ResolvedGameChoice::Preset {
            name,
            max_turns_override,
        } => GameSection {
            preset: Some(name.clone()),
            max_turns: max_turns_override.map(|n| n.get()),
            ..Default::default()
        },
        ResolvedGameChoice::Custom {
            width,
            height,
            cheese,
            symmetric,
            max_turns,
        } => GameSection {
            width: Some(*width),
            height: Some(*height),
            cheese: Some(*cheese),
            symmetric: Some(*symmetric),
            max_turns: max_turns.map(|n| n.get()),
            ..Default::default()
        },
    }
}

fn player_entry_from(player: &ResolvedPlayer, save_dir: &Path) -> PlayerEntry {
    use pyrat_orchestrator::PlayerSpec;
    let (command, working_dir) = match &player.spec {
        PlayerSpec::Subprocess {
            command,
            working_dir,
            ..
        } => (
            command.clone(),
            working_dir
                .as_ref()
                .map(|p| make_relative_or_absolute(p, save_dir)),
        ),
        // Embedded bots can't be serialized (factories are closures);
        // fall through to an empty command, which the resolver will
        // reject on reload. A user who builds an embedded-bot tournament
        // and then asks for --save-as is misusing the surface; document
        // this once we have a real embedded-bot path.
        _ => (String::new(), None),
    };
    PlayerEntry {
        id: player.id.clone(),
        command,
        working_dir,
    }
}

/// Best-effort absolute form: if the path's parent exists, canonicalize
/// it and rejoin the filename. If not, fall back to the path as-is.
/// Used as the base step in path rebasing so non-existent targets
/// (`store_path`, `replay_dir`) don't blow up `canonicalize`.
fn absolutize_path(path: &Path) -> PathBuf {
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        if let Ok(canon_parent) = fs::canonicalize(parent) {
            return canon_parent.join(name);
        }
    }
    path.to_path_buf()
}

/// Rebase `path` relative to `save_dir`, walking up with `..` when the
/// two live in sibling or unrelated subtrees. Without this, a
/// `--save-as configs/ladder.toml` from the repo root would serialize
/// `working_dir = "/absolute/path/to/botpack/greedy"` because `botpack/`
/// and `configs/` don't share a strip_prefix-compatible prefix — and
/// the saved spec would then only work on the author's machine.
///
/// Equivalent to `pathdiff::diff_paths`; ~15 lines of std, no new dep.
fn make_relative_or_absolute(path: &Path, save_dir: &Path) -> PathBuf {
    let abs_path = absolutize_path(path);
    let abs_save_dir = fs::canonicalize(save_dir).unwrap_or_else(|_| save_dir.to_path_buf());
    make_relative_to(&abs_path, &abs_save_dir)
}

/// Compute a relative path from `base` to `target`. Both paths must be
/// absolute and already canonicalized (or absolutized) — callers handle
/// that step so this function is purely structural.
fn make_relative_to(target: &Path, base: &Path) -> PathBuf {
    use std::path::Component;
    let target_components: Vec<Component<'_>> = target.components().collect();
    let base_components: Vec<Component<'_>> = base.components().collect();
    let common = target_components
        .iter()
        .zip(base_components.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut result = PathBuf::new();
    for _ in &base_components[common..] {
        result.push("..");
    }
    for c in &target_components[common..] {
        result.push(c.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

// ── Standings rendering (Level A) ────────────────────────────────────

#[derive(Serialize)]
struct ResultsJson<'a> {
    tournament_id: i64,
    status: &'a str,
    attempts: AttemptsCount,
    standings: Vec<StandingsRow<'a>>,
}

#[derive(Serialize)]
struct AttemptsCount {
    success: u64,
    failure: u64,
}

#[derive(Serialize)]
struct StandingsRow<'a> {
    player_id: &'a str,
    elo: f64,
}

fn render_standings(
    state: &TournamentState,
    results_json_path: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let counts = count_attempts(state);
    print_table(state, &counts);
    if let Some(path) = results_json_path {
        write_results_json(state, &counts, path)?;
    }
    Ok(())
}

fn count_attempts(state: &TournamentState) -> AttemptsCount {
    let mut success = 0u64;
    let mut failure = 0u64;
    for attempts in state.history.values() {
        for a in attempts {
            match a.outcome {
                MatchupOutcome::Success { .. } => success += 1,
                MatchupOutcome::Failure => failure += 1,
            }
        }
    }
    AttemptsCount { success, failure }
}

fn print_table(state: &TournamentState, counts: &AttemptsCount) {
    let TournamentId(id) = state.tournament_id;
    println!("Tournament {id} — finished");
    println!(
        "Attempts: {} success, {} failure",
        counts.success, counts.failure
    );
    println!();
    let mut standings = state.standings.clone();
    if standings.is_empty() {
        // Names the cause so the user knows where to look. The common case
        // is `success == 0 && failure > 0` (every matchup hit the retry
        // budget); flagging "Elo needs at least one success per matchup"
        // makes that actionable.
        println!(
            "No standings yet — {} successful attempts ({} failed). Elo needs at least one success per matchup.",
            counts.success, counts.failure
        );
        return;
    }
    standings.sort_by(|a, b| {
        b.elo
            .partial_cmp(&a.elo)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let max_id = standings
        .iter()
        .map(|r| r.player_id.len())
        .max()
        .unwrap_or(9);
    let id_width = max_id.max("player_id".len());
    println!("  rank  {:<id_width$}      elo", "player_id");
    for (i, rating) in standings.iter().enumerate() {
        println!(
            "  {:>4}  {:<id_width$}  {:>8.1}",
            i + 1,
            rating.player_id,
            rating.elo
        );
    }
}

fn write_results_json(
    state: &TournamentState,
    counts: &AttemptsCount,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let TournamentId(id) = state.tournament_id;
    let mut standings = state.standings.clone();
    standings.sort_by(|a, b| {
        b.elo
            .partial_cmp(&a.elo)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let rows: Vec<_> = standings
        .iter()
        .map(|r| StandingsRow {
            player_id: &r.player_id,
            elo: r.elo,
        })
        .collect();
    let payload = ResultsJson {
        tournament_id: id,
        status: "finished",
        attempts: AttemptsCount {
            success: counts.success,
            failure: counts.failure,
        },
        standings: rows,
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "--results-json: failed to create parent directory {}: {e}",
                    parent.display()
                )
            })?;
        }
    }
    let serialized = serde_json::to_string_pretty(&payload)?;
    fs::write(path, serialized)
        .map_err(|e| format!("--results-json: failed to write {}: {e}", path.display()))?;
    Ok(())
}

fn split_gauntlet_players(
    players: &[ResolvedPlayer],
    challenger_id: &str,
    opponent_ids: &[String],
) -> Result<(ResolvedPlayer, Vec<ResolvedPlayer>), Box<dyn std::error::Error>> {
    let find = |id: &str| -> Result<ResolvedPlayer, Box<dyn std::error::Error>> {
        players
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| format!("player `{id}` missing from player list").into())
    };
    let challenger = find(challenger_id)?;
    let opponents = opponent_ids
        .iter()
        .map(|id| find(id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((challenger, opponents))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tournament_resolve::ResolvedTiming;
    use pyrat_orchestrator::PlayerSpec;
    use std::num::NonZeroU16;

    fn fixture_resolved(save_as: Option<PathBuf>) -> ResolvedRun {
        ResolvedRun {
            players: vec![ResolvedPlayer {
                id: "greedy".into(),
                spec: PlayerSpec::Subprocess {
                    agent_id: "greedy".into(),
                    command: "cargo run --release".into(),
                    working_dir: Some(PathBuf::from("/tmp/work/botpack/greedy")),
                },
            }],
            game: ResolvedGameChoice::Preset {
                name: "tiny".into(),
                max_turns_override: NonZeroU16::new(50),
            },
            timing: ResolvedTiming {
                move_timeout_ms: 1000,
                preprocessing_timeout_ms: 10_000,
                startup_timeout_ms: 30_000,
                configure_timeout_ms: 5000,
                network_grace_ms: 50,
            },
            format: FormatChoice::RoundRobin,
            target_games_per_matchup: 5,
            max_failures_per_pair: 1,
            max_parallel: 2,
            seed: SeedSource::Generated(123),
            store_path: PathBuf::from("/tmp/work/ratings.db"),
            replay_dir: None,
            anchor: "greedy".into(),
            anchor_elo: 1000.0,
            results_json: None,
            save_as,
            resume: None,
        }
    }

    #[test]
    fn save_as_omits_implicit_seed() {
        let resolved = fixture_resolved(Some(PathBuf::from("/tmp/out.toml")));
        let cfg = to_saveable_config(&resolved, Path::new("/tmp"));
        assert!(cfg.seed.is_none(), "Generated seed should not be saved");
    }

    #[test]
    fn save_as_keeps_explicit_seed() {
        let mut resolved = fixture_resolved(Some(PathBuf::from("/tmp/out.toml")));
        resolved.seed = SeedSource::Explicit(42);
        let cfg = to_saveable_config(&resolved, Path::new("/tmp"));
        assert_eq!(cfg.seed, Some(42));
    }

    #[test]
    fn save_as_omits_from_store_on_resume_seed() {
        let mut resolved = fixture_resolved(Some(PathBuf::from("/tmp/out.toml")));
        resolved.seed = SeedSource::FromStoreOnResume;
        let cfg = to_saveable_config(&resolved, Path::new("/tmp"));
        assert!(cfg.seed.is_none());
    }

    #[test]
    fn save_as_rebases_paths_within_save_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bot_dir = tmp.path().join("bots/greedy");
        std::fs::create_dir_all(&bot_dir).unwrap();
        let store = tmp.path().join("ratings.db");

        let resolved = ResolvedRun {
            players: vec![ResolvedPlayer {
                id: "greedy".into(),
                spec: PlayerSpec::Subprocess {
                    agent_id: "greedy".into(),
                    command: "cargo run --release".into(),
                    working_dir: Some(bot_dir.clone()),
                },
            }],
            store_path: store.clone(),
            ..fixture_resolved(None)
        };
        let cfg = to_saveable_config(&resolved, tmp.path());

        // Both paths should be relative to save_dir.
        assert_eq!(cfg.store_path.as_deref(), Some(Path::new("ratings.db")));
        assert_eq!(
            cfg.players[0].working_dir.as_deref(),
            Some(Path::new("bots/greedy"))
        );
    }

    #[test]
    fn save_as_rebases_paths_outside_save_dir_with_dotdot() {
        // save_dir and bot_dir live in unrelated tempdirs (sibling-ish
        // under /var/folders or /tmp). The rebased path must walk up
        // with `..` and back down — without this, the saved TOML would
        // contain absolute paths that only work on the original machine.
        let save_dir = tempfile::tempdir().expect("save dir");
        let bots_root = tempfile::tempdir().expect("bots dir");
        let bot_dir = bots_root.path().join("greedy");
        std::fs::create_dir_all(&bot_dir).unwrap();

        let resolved = ResolvedRun {
            players: vec![ResolvedPlayer {
                id: "greedy".into(),
                spec: PlayerSpec::Subprocess {
                    agent_id: "greedy".into(),
                    command: "cargo run --release".into(),
                    working_dir: Some(bot_dir.clone()),
                },
            }],
            store_path: bot_dir.join("ratings.db"),
            ..fixture_resolved(None)
        };
        let cfg = to_saveable_config(&resolved, save_dir.path());

        // Round-trip: the relative working_dir, joined to save_dir,
        // must canonicalize back to bot_dir.
        let written = cfg.players[0]
            .working_dir
            .as_ref()
            .expect("working_dir present");
        let resolved_back =
            std::fs::canonicalize(save_dir.path().join(written)).expect("canonicalize round-trip");
        let bot_dir_canon = std::fs::canonicalize(&bot_dir).expect("canonicalize bot_dir");
        assert_eq!(resolved_back, bot_dir_canon);
        // And the path should contain `..` since it crosses subtrees.
        assert!(
            written
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
            "expected `..` in rebased path, got: {written:?}",
        );
    }

    #[test]
    fn make_relative_to_sibling_directory() {
        let target = Path::new("/tmp/botpack/greedy");
        let base = Path::new("/tmp/configs");
        assert_eq!(
            make_relative_to(target, base),
            PathBuf::from("../botpack/greedy")
        );
    }

    #[test]
    fn make_relative_to_nested_directory() {
        let target = Path::new("/tmp/configs/foo");
        let base = Path::new("/tmp");
        assert_eq!(make_relative_to(target, base), PathBuf::from("configs/foo"));
    }

    #[test]
    fn make_relative_to_parent_directory() {
        let target = Path::new("/tmp/a");
        let base = Path::new("/tmp/a/b");
        assert_eq!(make_relative_to(target, base), PathBuf::from(".."));
    }

    #[test]
    fn make_relative_to_same_directory_is_dot() {
        let target = Path::new("/tmp/a");
        let base = Path::new("/tmp/a");
        assert_eq!(make_relative_to(target, base), PathBuf::from("."));
    }

    #[test]
    fn save_as_handles_nonexistent_store_path() {
        // store_path points at a file inside save_dir that doesn't exist
        // yet (typical first-run scenario). The serializer must not panic
        // on canonicalize-of-missing.
        let save_dir = tempfile::tempdir().expect("save dir");
        let nonexistent_store = save_dir.path().join("not-yet/ratings.db");
        std::fs::create_dir_all(nonexistent_store.parent().unwrap()).unwrap();

        let resolved = ResolvedRun {
            store_path: nonexistent_store.clone(),
            ..fixture_resolved(None)
        };
        let cfg = to_saveable_config(&resolved, save_dir.path());

        // Should produce a relative path.
        assert_eq!(
            cfg.store_path.as_deref(),
            Some(Path::new("not-yet/ratings.db"))
        );
    }

    #[test]
    fn count_attempts_counts_success_and_failure_separately() {
        use pyrat_eval::{MatchupKey, MatchupOutcome, TournamentState};
        let mut state = TournamentState::empty(TournamentId(7));
        let key = MatchupKey::from_pair("a", "b", "gc", 0);
        state.history.insert(
            key,
            vec![
                pyrat_eval::MatchupAttempt {
                    attempt_index: 0,
                    outcome: MatchupOutcome::Success {
                        player1_score: 1.0,
                        player2_score: 0.0,
                    },
                },
                pyrat_eval::MatchupAttempt {
                    attempt_index: 1,
                    outcome: MatchupOutcome::Failure,
                },
                pyrat_eval::MatchupAttempt {
                    attempt_index: 2,
                    outcome: MatchupOutcome::Success {
                        player1_score: 0.5,
                        player2_score: 0.5,
                    },
                },
            ],
        );
        let counts = count_attempts(&state);
        assert_eq!(counts.success, 2);
        assert_eq!(counts.failure, 1);
    }

    #[test]
    fn results_json_includes_tournament_id_and_standings_descending() {
        use pyrat_eval::TournamentState;
        use pyrat_eval_store::EloRating;
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("results.json");

        let mut state = TournamentState::empty(TournamentId(11));
        state.standings = vec![
            EloRating {
                player_id: "loser".into(),
                elo: 800.0,
            },
            EloRating {
                player_id: "winner".into(),
                elo: 1200.0,
            },
        ];
        let counts = AttemptsCount {
            success: 4,
            failure: 1,
        };

        write_results_json(&state, &counts, &path).expect("write");
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();

        assert_eq!(parsed["tournament_id"], 11);
        assert_eq!(parsed["status"], "finished");
        assert_eq!(parsed["attempts"]["success"], 4);
        assert_eq!(parsed["attempts"]["failure"], 1);
        // Standings sorted descending by elo.
        assert_eq!(parsed["standings"][0]["player_id"], "winner");
        assert_eq!(parsed["standings"][1]["player_id"], "loser");
    }

    #[test]
    fn results_json_emits_empty_standings_when_elo_unavailable() {
        use pyrat_eval::TournamentState;
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("results.json");
        let state = TournamentState::empty(TournamentId(0));
        let counts = AttemptsCount {
            success: 0,
            failure: 0,
        };
        write_results_json(&state, &counts, &path).expect("write");
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed["standings"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn write_save_as_round_trips_through_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bot_dir = tmp.path().join("bots/greedy");
        std::fs::create_dir_all(&bot_dir).unwrap();
        let save_path = tmp.path().join("ladder.toml");

        let resolved = ResolvedRun {
            players: vec![ResolvedPlayer {
                id: "greedy".into(),
                spec: PlayerSpec::Subprocess {
                    agent_id: "greedy".into(),
                    command: "cargo run --release".into(),
                    working_dir: Some(bot_dir),
                },
            }],
            store_path: tmp.path().join("ratings.db"),
            save_as: Some(save_path.clone()),
            ..fixture_resolved(None)
        };

        write_save_as(&resolved, &save_path).expect("write");

        let raw = std::fs::read_to_string(&save_path).expect("read back");
        let parsed: TournamentConfig = toml::from_str(&raw).expect("parse");
        assert_eq!(parsed.format.as_deref(), Some("round_robin"));
        assert_eq!(parsed.target_games_per_matchup, Some(5));
        assert_eq!(parsed.players[0].id, "greedy");
        assert!(parsed.seed.is_none(), "Generated seed must not appear");
    }
}
