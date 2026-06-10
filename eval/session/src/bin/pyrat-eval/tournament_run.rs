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
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use pyrat_eval::MatchupOutcome;
use pyrat_eval::{
    gauntlet_slot_order, EvalMatchDescriptor, EvalSession, GauntletPlanner,
    GauntletPlannerConfig, Planner, ResolvedPlayer, RoundRobinPlanner, RoundRobinPlannerConfig,
    SessionConfig, SessionError, SessionMode, TournamentMismatch, TournamentParams,
    TournamentSpec, TournamentState,
};
use pyrat_eval_store::{EloOptions, EvalStore, TournamentId};
use pyrat_host::wire::TimingMode;
use pyrat_orchestrator::{DirectoryWriter, MatchSink, ReplaySink, SinkRole, Timing};
use serde::Serialize;

use crate::game_config_build::build_game_config;
use crate::orchestrator_config_build::build_orchestrator_config;
use crate::tournament_resolve::{FormatChoice, LaunchMode, ResolvedRun};
use crate::tournament_save::write_save_as;

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
    let (tournament_id, game_config_id, tournament_seed) = match resolved.mode {
        LaunchMode::Resume { id, seed_assert } => {
            let (id, gc_id, seed) =
                realize_resume(&store, id, seed_assert, &resolved.store_path)?;
            validate_game_config_with_seed(&game_config, seed)?;
            (id, gc_id, seed)
        },
        LaunchMode::New { seed } => {
            let seed = seed.value();
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
    let session = match EvalSession::start_with_extra_sinks(
        store.clone(),
        SessionMode { tournament_id },
        planner,
        orch_config,
        build_elo_options(&resolved),
        SessionConfig::default(),
        extra_sinks,
    )
    .await
    {
        // On resume, mismatches come from the user's flags/config
        // diverging from the stored tournament — translate the library
        // vocabulary into the flags they actually typed.
        Err(SessionError::TournamentMismatch(m))
            if matches!(resolved.mode, LaunchMode::Resume { .. }) =>
        {
            return Err(format!("resume rejected: {}", translate_mismatch(&m)).into());
        },
        other => other?,
    };
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

/// Bootstrap a fresh tournament and return its identity. Gauntlet slot
/// ordering comes from `gauntlet_slot_order` — the same function the
/// planner's `expected_players()` uses, so create and resume-validation
/// agree by construction.
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
            let (c, ops) = split_gauntlet_players(&resolved.players, challenger, opponents)?;
            let v: Vec<ResolvedPlayer> = gauntlet_slot_order(&c, &ops).cloned().collect();
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

/// On resume, the store carries the seed and game_config_id. An explicit
/// seed is only an assertion: validate it against the stored value
/// before the bots launch so users get a clear error rather than a
/// cryptic `TournamentMismatch` from the planner guard.
fn realize_resume(
    store: &Arc<Mutex<EvalStore>>,
    id: TournamentId,
    seed_assert: Option<u64>,
    store_path: &Path,
) -> Result<(TournamentId, String, u64), Box<dyn std::error::Error>> {
    let stored = {
        let store = store.lock();
        store.get_tournament(id)?.ok_or_else(|| {
            // The likely cause is resuming against the wrong store file
            // (e.g. forgot --config, so the default ratings.db in CWD
            // was opened) — naming the path makes that visible.
            format!("tournament {} not found in {}", id.0, store_path.display())
        })?
    };
    let seed = match seed_assert {
        Some(s) => {
            if s != stored.tournament_seed {
                return Err(format!(
                    "seed mismatch on resume: explicit seed {} (from --seed or config) does not match stored {} (tournament {})",
                    s, stored.tournament_seed, id.0
                )
                .into());
            }
            s
        },
        None => stored.tournament_seed,
    };
    Ok((id, stored.game_config_id, seed))
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

/// Map the library's mismatch vocabulary onto CLI flags. The library
/// `Display` (field names like `target_games_per_matchup`) stays intact
/// for library consumers; CLI users typed `--games`.
fn translate_mismatch(m: &TournamentMismatch) -> String {
    match m {
        TournamentMismatch::TargetPerPair { planner, stored } => format!(
            "--games {planner} does not match the stored target {stored}; rerun with --games {stored}"
        ),
        TournamentMismatch::Params { planner, stored } => format!(
            "--max-failures {} does not match the stored value {}; rerun with --max-failures {}",
            planner.max_failures_per_pair,
            stored.max_failures_per_pair,
            stored.max_failures_per_pair
        ),
        TournamentMismatch::Format { planner, stored } => {
            format!("--format {planner} does not match the stored format {stored}")
        },
        TournamentMismatch::Players { planner, stored } => format!(
            "player set {planner:?} does not match stored players {stored:?} (check --bot / --challenger / --opponent or [[players]])"
        ),
        TournamentMismatch::GameConfigDrift { .. } => format!(
            "{m}\nadjust --preset/--width/--height/--cheese/--max-turns (or [game]) to match the stored config"
        ),
        // TournamentId / GameConfigId / Seed / Invalid: the library form
        // is already the clearest rendering (and Seed is normally
        // pre-empted by realize_resume's own check).
        other => other.to_string(),
    }
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

    /// The CLI translation names the flags the user typed, with the
    /// stored value to rerun with.
    #[test]
    fn translate_mismatch_names_cli_flags() {
        let target = TournamentMismatch::TargetPerPair {
            planner: 5,
            stored: 1,
        };
        let s = translate_mismatch(&target);
        assert!(s.contains("--games 5"), "got: {s}");
        assert!(s.contains("rerun with --games 1"), "got: {s}");

        let params = TournamentMismatch::Params {
            planner: TournamentParams {
                max_failures_per_pair: 3,
            },
            stored: TournamentParams {
                max_failures_per_pair: 1,
            },
        };
        let s = translate_mismatch(&params);
        assert!(s.contains("--max-failures 3"), "got: {s}");
        assert!(s.contains("rerun with --max-failures 1"), "got: {s}");
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

}
