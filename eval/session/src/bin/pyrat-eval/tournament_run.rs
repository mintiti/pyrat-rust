//! Tournament run executor. Takes a `ResolvedRun`, wires the store +
//! planner + sinks, and drives the session to completion.
//!
//! Two paths:
//! - **New**: bootstrap the tournament via `EvalSession::create_tournament`,
//!   get the `(tournament_id, game_config_id)` pair, build the planner,
//!   start.
//! - **Resume (`--resume <id>`)**: query the store for the existing
//!   tournament, reuse its `game_config_id` and `tournament_seed`, build
//!   the planner matching the stored spec, start. The spec itself is
//!   re-derived from the user's flags/config and *verified* against the
//!   store (the store carries results and identity, not bot commands).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use pyrat_eval::MatchupOutcome;
use pyrat_eval::{
    gauntlet_slot_order, EvalMatchDescriptor, EvalSession, GauntletPlanner, GauntletPlannerConfig,
    Planner, ResolvedPlayer, RoundRobinPlanner, RoundRobinPlannerConfig, SessionConfig,
    SessionError, SessionMode, TournamentMismatch, TournamentParams, TournamentSpec,
    TournamentState,
};
use pyrat_eval_store::{compute_elo_with_uncertainty, EloOptions, EvalStore, TournamentId};
use pyrat_host::wire::TimingMode;
use pyrat_orchestrator::{DirectoryWriter, MatchSink, ReplaySink, SinkRole, Timing};
use serde::Serialize;

use crate::game_config_build::build_game_config;
use crate::orchestrator_config_build::build_orchestrator_config;
use crate::tournament_resolve::{FormatChoice, LaunchMode, ResolvedRun};
use crate::tournament_save::write_save_as;

/// Execute the tournament described by `resolved`. Returns the attempt
/// counts so the caller can derive an exit code (rendering — standings
/// table, results JSON — happens in here).
///
/// If `resolved.save_as` is set, materializes the spec to TOML
/// **before** the tournament runs so the user gets the spec file even
/// if the run aborts.
pub async fn run_tournament_main(
    resolved: ResolvedRun,
) -> Result<AttemptsCount, Box<dyn std::error::Error>> {
    if let Some(save_path) = resolved.save_as.as_ref() {
        write_save_as(&resolved, save_path)?;
    }

    let game_config = build_game_config(&resolved.game)?;
    // The store path is often implicit (config stem, or ./ratings.db);
    // an open failure must name it or the user can't tell what broke.
    let store = EvalStore::open(&resolved.store_path).map_err(|e| {
        format!(
            "failed to open store {}: {e}",
            resolved.store_path.display()
        )
    })?;
    let store = Arc::new(Mutex::new(store));

    // Realize the seed and the (tournament_id, game_config_id) pair. On
    // resume, the store is the source of truth; on a new tournament,
    // we validate the game config *before* bootstrap so an invalid
    // config (e.g. too much cheese for the board) doesn't leave a
    // dangling tournament row behind.
    let (tournament_id, game_config_id, tournament_seed) = match resolved.mode {
        LaunchMode::Resume { id, seed_assert } => {
            let (id, gc_id, seed) = realize_resume(&store, id, seed_assert, &resolved.store_path)?;
            validate_game_config_with_seed(&game_config, seed)?;
            (id, gc_id, seed)
        },
        LaunchMode::New { seed } => {
            let seed = seed.value();
            validate_game_config_with_seed(&game_config, seed)?;
            bootstrap_new(&store, &resolved, &game_config, seed).await?
        },
    };

    // Operational guidance, not tracing: the id is what --resume needs
    // after an abort, so it must survive RUST_LOG filtering. stderr
    // keeps stdout machine-clean for the standings/JSON consumers.
    eprintln!(
        "tournament {} started (store: {})",
        tournament_id.0,
        resolved.store_path.display()
    );

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
        // vocabulary into the flags they actually typed. (No resume
        // hint here: a rejected resume ran nothing.)
        Err(SessionError::TournamentMismatch(m))
            if matches!(resolved.mode, LaunchMode::Resume { .. }) =>
        {
            return Err(format!("resume rejected: {}", translate_mismatch(&m)).into());
        },
        Err(e) => {
            return Err(
                with_resume_hint(&e.to_string(), &resolved.store_path, tournament_id).into(),
            )
        },
        Ok(session) => session,
    };
    let final_state = await_session(session)
        .await
        .map_err(|e| with_resume_hint(&e.to_string(), &resolved.store_path, tournament_id))?;

    render_standings(
        &final_state,
        resolved.results_json.as_deref(),
        &build_elo_options(&resolved),
    )
}

/// Append resume guidance to a session-phase error. By this point the
/// tournament row exists and any completed games are durably in the
/// store, so an aborted run is resumable — but only if the user knows
/// the id and where the store lives.
fn with_resume_hint(e: &str, store_path: &Path, id: TournamentId) -> String {
    format!(
        "{e}\npartial results in {}; resume with --resume {}",
        store_path.display(),
        id.0
    )
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
    standings: &'a [StandingsEntry],
}

#[derive(Serialize)]
pub(crate) struct AttemptsCount {
    pub(crate) success: u64,
    pub(crate) failure: u64,
}

/// One final-standings row: Elo with its standard error (conditional on
/// the anchor, whose own stderr is near-zero) and games played.
#[derive(Serialize)]
struct StandingsEntry {
    player_id: String,
    elo: f64,
    elo_stderr: f64,
    games: u32,
}

/// Recompute Elo from history with uncertainty, once, at render time.
/// The session's live standings deliberately skip the covariance work
/// (they refresh on every MatchFinished); the final table pays for it
/// a single time here. Same history, same options — identical ratings.
fn compute_final_standings(state: &TournamentState, options: &EloOptions) -> Vec<StandingsEntry> {
    let h2h = state.head_to_head();
    let Ok((result, uncertainty)) = compute_elo_with_uncertainty(&h2h, options) else {
        // Mirrors the session's recompute: no records or a disconnected
        // player graph means no standings (the caller prints the hint).
        return Vec::new();
    };
    let mut games: HashMap<&str, u32> = HashMap::new();
    for r in &h2h {
        *games.entry(r.player_a.as_str()).or_default() += r.total();
        *games.entry(r.player_b.as_str()).or_default() += r.total();
    }
    let mut entries: Vec<StandingsEntry> = result
        .ratings
        .iter()
        .map(|r| StandingsEntry {
            player_id: r.player_id.clone(),
            elo: r.elo,
            elo_stderr: uncertainty.stderr(&r.player_id).unwrap_or(0.0),
            games: games.get(r.player_id.as_str()).copied().unwrap_or(0),
        })
        .collect();
    entries.sort_by(|a, b| {
        b.elo
            .partial_cmp(&a.elo)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    entries
}

fn render_standings(
    state: &TournamentState,
    results_json_path: Option<&Path>,
    elo_options: &EloOptions,
) -> Result<AttemptsCount, Box<dyn std::error::Error>> {
    let counts = count_attempts(state);
    let standings = compute_final_standings(state, elo_options);
    print_table(state, &counts, &standings);
    if let Some(path) = results_json_path {
        write_results_json(state, &counts, &standings, path)?;
    }
    Ok(counts)
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

fn print_table(state: &TournamentState, counts: &AttemptsCount, standings: &[StandingsEntry]) {
    let TournamentId(id) = state.tournament_id;
    println!("Tournament {id} — finished");
    println!(
        "Attempts: {} success, {} failure",
        counts.success, counts.failure
    );
    println!();
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
    let max_id = standings
        .iter()
        .map(|r| r.player_id.len())
        .max()
        .unwrap_or(9);
    let id_width = max_id.max("player_id".len());
    println!("  rank  {:<id_width$}      elo    ±err  games", "player_id");
    for (i, r) in standings.iter().enumerate() {
        println!(
            "  {:>4}  {:<id_width$}  {:>8.1}  {:>6.1}  {:>5}",
            i + 1,
            r.player_id,
            r.elo,
            r.elo_stderr,
            r.games
        );
    }
}

fn write_results_json(
    state: &TournamentState,
    counts: &AttemptsCount,
    standings: &[StandingsEntry],
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let TournamentId(id) = state.tournament_id;
    // "finished" with zero successes means every matchup exhausted its
    // retry budget — a script consuming this JSON must be able to tell
    // that apart from a real result set.
    let status = if counts.success == 0 {
        "finished_no_results"
    } else {
        "finished"
    };
    let payload = ResultsJson {
        tournament_id: id,
        status,
        attempts: AttemptsCount {
            success: counts.success,
            failure: counts.failure,
        },
        standings,
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

    /// The hint carries everything a user needs to pick the run back
    /// up: the store location and the exact --resume invocation.
    #[test]
    fn with_resume_hint_names_store_and_id() {
        let hinted = with_resume_hint(
            "orchestrator error: boom",
            Path::new("/tmp/ratings.db"),
            TournamentId(7),
        );
        assert!(
            hinted.starts_with("orchestrator error: boom\n"),
            "got: {hinted}"
        );
        assert!(
            hinted.contains("partial results in /tmp/ratings.db"),
            "got: {hinted}"
        );
        assert!(hinted.contains("--resume 7"), "got: {hinted}");
    }

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
        use pyrat_eval::{MatchupKey, MatchupOutcome, TournamentState};
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("results.json");

        let mut state = TournamentState::empty(TournamentId(11));
        // Canonical key order is lexicographic: player1 = "loser". Three
        // winner wins, one draw, one failure (skipped by Elo and games).
        let key = MatchupKey::from_pair("winner", "loser", "gc", 0);
        let success = |s1: f64, s2: f64| pyrat_eval::MatchupAttempt {
            attempt_index: 0,
            outcome: MatchupOutcome::Success {
                player1_score: s1,
                player2_score: s2,
            },
        };
        state.history.insert(
            key,
            vec![
                success(0.0, 1.0),
                success(0.0, 1.0),
                success(0.0, 1.0),
                success(0.5, 0.5),
                pyrat_eval::MatchupAttempt {
                    attempt_index: 4,
                    outcome: MatchupOutcome::Failure,
                },
            ],
        );
        let counts = count_attempts(&state);
        let options = EloOptions::new("loser".to_string()).anchor_elo(1000.0);
        let standings = compute_final_standings(&state, &options);

        write_results_json(&state, &counts, &standings, &path).expect("write");
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();

        assert_eq!(parsed["tournament_id"], 11);
        assert_eq!(parsed["status"], "finished");
        assert_eq!(parsed["attempts"]["success"], 4);
        assert_eq!(parsed["attempts"]["failure"], 1);
        // Standings sorted descending by elo.
        assert_eq!(parsed["standings"][0]["player_id"], "winner");
        assert_eq!(parsed["standings"][1]["player_id"], "loser");
        // Anchor pins at 1000; the winner sits above it.
        assert_eq!(parsed["standings"][1]["elo"], 1000.0);
        assert!(parsed["standings"][0]["elo"].as_f64().unwrap() > 1000.0);
        // Uncertainty: the anchor's stderr is near-zero (it's the fixed
        // reference), the winner's is meaningfully larger. Games count
        // successes only.
        let anchor_err = parsed["standings"][1]["elo_stderr"].as_f64().unwrap();
        let winner_err = parsed["standings"][0]["elo_stderr"].as_f64().unwrap();
        assert!(anchor_err < 1.0, "anchor stderr ~0, got {anchor_err}");
        assert!(winner_err > anchor_err);
        assert_eq!(parsed["standings"][0]["games"], 4);
        assert_eq!(parsed["standings"][1]["games"], 4);
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
        let options = EloOptions::new("anyone".to_string()).anchor_elo(1000.0);
        let standings = compute_final_standings(&state, &options);
        assert!(standings.is_empty());
        write_results_json(&state, &counts, &standings, &path).expect("write");
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed["standings"].as_array().unwrap().len(), 0);
        // Zero successes must be distinguishable from a real result set.
        assert_eq!(parsed["status"], "finished_no_results");
    }
}
