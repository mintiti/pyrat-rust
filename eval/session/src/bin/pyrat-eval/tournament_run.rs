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

use std::sync::Arc;

use parking_lot::Mutex;
use pyrat_eval::{
    EvalMatchDescriptor, EvalSession, GauntletPlanner, GauntletPlannerConfig, ResolvedPlayer,
    RoundRobinPlanner, RoundRobinPlannerConfig, SessionConfig, SessionMode, TournamentSpec,
    TournamentState,
};
use pyrat_eval_store::{EvalStore, TournamentId};
use pyrat_host::wire::TimingMode;
use pyrat_orchestrator::{DirectoryWriter, MatchSink, ReplaySink, SinkRole, Timing};

use crate::game_config_build::build_game_config;
use crate::orchestrator_config_build::build_orchestrator_config;
use crate::tournament_resolve::{FormatChoice, ResolvedRun, SeedSource};

/// Execute the tournament described by `resolved`. Returns the final
/// `TournamentState` for the caller to render (standings, JSON).
pub async fn run_tournament_main(
    resolved: ResolvedRun,
) -> Result<TournamentState, Box<dyn std::error::Error>> {
    let game_config = build_game_config(&resolved.game)?;
    let store = Arc::new(Mutex::new(EvalStore::open(&resolved.store_path)?));

    // Realize the seed and the (tournament_id, game_config_id) pair. On
    // resume, the store is the source of truth; on a new tournament,
    // create_tournament returns both.
    let (tournament_id, game_config_id, tournament_seed) = match resolved.resume {
        Some(id) => realize_resume(&store, id, &resolved.seed)?,
        None => {
            let seed = match resolved.seed {
                SeedSource::Explicit(s) | SeedSource::Generated(s) => s,
                SeedSource::FromStoreOnResume => {
                    return Err("internal: FromStoreOnResume on non-resume path".into())
                },
            };
            bootstrap_new(&store, &resolved, &game_config, seed).await?
        },
    };

    // Pre-spawn validation. Fail before bots launch if the game config
    // refuses the resolved seed (e.g. too many cheese for the board).
    game_config
        .clone()
        .create(Some(tournament_seed))
        .map_err(|e| format!("invalid game config: {e}"))?;

    let orch_config = build_orchestrator_config(&resolved.timing, resolved.max_parallel);
    let per_match_timing = Timing {
        mode: TimingMode::Wait,
        move_timeout_ms: resolved.timing.move_timeout_ms,
        preprocessing_timeout_ms: resolved.timing.preprocessing_timeout_ms,
    };

    // Extra sinks: optional ReplaySink in the configured directory.
    let mut extra_sinks: Vec<(SinkRole, Arc<dyn MatchSink<EvalMatchDescriptor>>)> = Vec::new();
    if let Some(dir) = resolved.replay_dir.as_ref() {
        let writer = Arc::new(DirectoryWriter::new(dir.clone())?);
        let replay: Arc<dyn MatchSink<EvalMatchDescriptor>> = Arc::new(
            ReplaySink::new(writer)
                .with_engine_version(format!("pyrat-eval/{}", env!("CARGO_PKG_VERSION"))),
        );
        extra_sinks.push((SinkRole::Optional, replay));
    }

    // Branch on planner type. Post-start logic (state capture, join) is
    // identical so factor it out via `await_session`.
    let final_state = match &resolved.format {
        FormatChoice::RoundRobin => {
            let planner = RoundRobinPlanner::new(RoundRobinPlannerConfig {
                players: resolved.players.clone(),
                game_config: game_config.clone(),
                game_config_id,
                timing: per_match_timing,
                tournament_id,
                target_per_pair: resolved.target_games_per_matchup,
                max_failures_per_pair: resolved.max_failures_per_pair,
                tournament_seed,
            });
            let session = EvalSession::start_with_extra_sinks(
                store.clone(),
                SessionMode { tournament_id },
                planner,
                orch_config,
                resolved.elo.clone(),
                SessionConfig::default(),
                extra_sinks,
            )
            .await?;
            await_session(session).await?
        },
        FormatChoice::Gauntlet {
            challenger,
            opponents,
        } => {
            let (challenger_p, opponent_ps) =
                split_gauntlet_players(&resolved.players, challenger, opponents)?;
            let planner = GauntletPlanner::new(GauntletPlannerConfig {
                challenger: challenger_p,
                opponents: opponent_ps,
                game_config: game_config.clone(),
                game_config_id,
                timing: per_match_timing,
                tournament_id,
                target_each: resolved.target_games_per_matchup,
                max_failures_per_pair: resolved.max_failures_per_pair,
                tournament_seed,
            });
            let session = EvalSession::start_with_extra_sinks(
                store.clone(),
                SessionMode { tournament_id },
                planner,
                orch_config,
                resolved.elo.clone(),
                SessionConfig::default(),
                extra_sinks,
            )
            .await?;
            await_session(session).await?
        },
    };

    Ok(final_state)
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
    let spec = TournamentSpec {
        format: format_str,
        target_games_per_matchup: Some(resolved.target_games_per_matchup),
        params_json: "{}".into(),
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
            .ok_or_else(|| format!("tournament {id:?} not found in store"))?
    };
    let seed = match seed_source {
        SeedSource::Explicit(s) => {
            if *s != stored.tournament_seed {
                return Err(format!(
                    "seed mismatch on resume: --seed {} does not match stored {} (tournament {:?})",
                    s, stored.tournament_seed, id
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

/// Pick the challenger and ordered opponents from the resolved player
/// pool. The resolver has already validated that every id exists; this
/// just slices the pool into the planner-expected shape.
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
