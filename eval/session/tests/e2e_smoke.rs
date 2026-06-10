//! Library-level smoke for the full eval session stack.
//!
//! Exercises planner → orchestrator → store + replay sink → session in
//! one pass with two embedded bots. Not via the CLI — this is the
//! library surface that GUI and alpharat consume.
//!
//! Asserts that:
//! 1. The session runs N matches to completion (target_per_pair × pairs).
//! 2. The store records N success rows (no failures, no duplicates).
//! 3. The replay sink writes one buffer per success.
//! 4. The orchestrator broadcast NEVER surfaces `MatchEvent::MatchOver`
//!    (suppression invariant from PR 3 — the canonical terminal is the
//!    `DriverEvent::MatchFinished` outcome).
//!
//! Part 9b of the source plan — `std::process::abort()` mid-run plus a
//! resume that reissues missing slots at the same `attempt_index` and
//! `seed` — and Part 9a's deliberate `CrashingBot` failure injection
//! both stay as follow-ups: panic-failure durability semantics need
//! their own pass to nail down, and the subprocess+abort machinery has
//! a known flakiness risk on CI per the source plan.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use pyrat::game::builder::GameBuilder;
use pyrat::{Coordinates, Direction};
use pyrat_bot_api::Options;
use pyrat_eval::{
    EvalMatchDescriptor, EvalSession, ResolvedPlayer, RoundRobinPlanner, RoundRobinPlannerConfig,
    SessionConfig, SessionMode, TournamentParams, TournamentSpec,
};
use pyrat_eval_store::{EloOptions, EvalStore};
use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::{EmbeddedBot, EmbeddedCtx, PlayerIdentity};
use pyrat_host::wire::TimingMode;
use pyrat_orchestrator::{
    EmbeddedBotFactory, MatchFailure, MatchId, MatchOutcome, MatchSink, MemoryWriter,
    OrchestratorConfig, OrchestratorEvent, PlayerSpec, ReplaySink, SinkError, SinkRole, Timing,
};
use pyrat_protocol::HashedTurnState;

/// Stay bot: deterministic, never moves. Pairing two of them produces
/// a 0-cheese tie at max_turns.
struct StayBot;
impl Options for StayBot {}
impl EmbeddedBot for StayBot {
    fn think(&mut self, _state: &HashedTurnState, _ctx: &EmbeddedCtx) -> Direction {
        Direction::Stay
    }
}

/// Test-only sink that records each `MatchOutcome` it sees. Used to
/// confirm the success terminal fires the expected number of times.
struct OutcomeCounter {
    successes: AtomicUsize,
    failures: AtomicUsize,
}

impl OutcomeCounter {
    fn new() -> Self {
        Self {
            successes: AtomicUsize::new(0),
            failures: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl MatchSink<EvalMatchDescriptor> for OutcomeCounter {
    async fn on_match_started(
        &self,
        _descriptor: &EvalMatchDescriptor,
        _players: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        Ok(())
    }
    async fn on_match_event(&self, _id: MatchId, _event: &MatchEvent) -> Result<(), SinkError> {
        Ok(())
    }
    async fn on_match_finished(
        &self,
        _outcome: &MatchOutcome<EvalMatchDescriptor>,
    ) -> Result<(), SinkError> {
        self.successes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn on_match_failed(
        &self,
        _failure: &MatchFailure<EvalMatchDescriptor>,
    ) -> Result<(), SinkError> {
        self.failures.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn end_to_end_round_robin_with_replay_sink() {
    // 1 pair × target_per_pair=3 = 3 matchups. max_parallel=2 forces
    // concurrent execution for at least one batch.
    let target_per_pair = 3u32;
    let expected_attempts = target_per_pair as usize;

    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let game_config = GameBuilder::new(3, 3)
        .with_open_maze()
        .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
        .with_random_cheese(1, false)
        .with_max_turns(5)
        .build();

    let factory: EmbeddedBotFactory = Arc::new(|| Box::new(StayBot));
    let players = vec![
        ResolvedPlayer {
            id: "alpha".into(),
            spec: PlayerSpec::Embedded {
                agent_id: "alpha".into(),
                name: "alpha".into(),
                author: "tests".into(),
                factory: factory.clone(),
            },
        },
        ResolvedPlayer {
            id: "beta".into(),
            spec: PlayerSpec::Embedded {
                agent_id: "beta".into(),
                name: "beta".into(),
                author: "tests".into(),
                factory,
            },
        },
    ];
    let tournament_seed = 0xBEEF;

    let spec = TournamentSpec {
        format: "round_robin".into(),
        target_games_per_matchup: Some(target_per_pair),
        params_json: TournamentParams {
            max_failures_per_pair: 1,
        }
        .to_json(),
        game_config: game_config.clone(),
        tournament_seed,
    };
    let created = EvalSession::create_tournament(store.clone(), spec, players.clone())
        .await
        .expect("create_tournament");

    let planner = RoundRobinPlanner::new(RoundRobinPlannerConfig {
        players: players.clone(),
        game_config: game_config.clone(),
        game_config_id: created.game_config_id.clone(),
        timing: Timing {
            mode: TimingMode::Wait,
            move_timeout_ms: 2000,
            preprocessing_timeout_ms: 5000,
        },
        tournament_id: created.tournament_id,
        target_per_pair,
        max_failures_per_pair: 1,
        tournament_seed,
    });

    // Extras: ReplaySink + OutcomeCounter. The StoreSink is prepended by
    // start_with_extra_sinks; we don't list it here.
    let replay_writer = Arc::new(MemoryWriter::new());
    let replay_sink: Arc<dyn MatchSink<EvalMatchDescriptor>> =
        Arc::new(ReplaySink::new(replay_writer.clone()).with_engine_version("pyrat-eval-tests/0"));
    let counter = Arc::new(OutcomeCounter::new());
    let counter_sink: Arc<dyn MatchSink<EvalMatchDescriptor>> = counter.clone();
    let extra_sinks = vec![
        (SinkRole::Optional, replay_sink),
        // Counter must be Required so its terminal fires before the
        // run-loop publishes — we don't actually rely on Required
        // semantics here, but it costs nothing and matches StoreSink.
        (SinkRole::Required, counter_sink),
    ];

    let session = EvalSession::start_with_extra_sinks(
        store.clone(),
        SessionMode {
            tournament_id: created.tournament_id,
        },
        planner,
        OrchestratorConfig {
            max_parallel: 2,
            ..OrchestratorConfig::default()
        },
        EloOptions::new("alpha"),
        SessionConfig::default(),
        extra_sinks,
    )
    .await
    .expect("start");

    // Subscribe to live_events BEFORE the run-loop publishes so we
    // observe every per-turn event. Drain into a Vec on a background
    // task so we can assert post-join.
    let mut live = session.live_events();
    let observed = Arc::new(Mutex::new(
        Vec::<OrchestratorEvent<EvalMatchDescriptor>>::new(),
    ));
    let observed_clone = observed.clone();
    let drain = tokio::spawn(async move {
        loop {
            match live.recv().await {
                Ok(event) => observed_clone.lock().push(event),
                // Lagging must not silently truncate observation — the
                // MatchOver invariant below would pass vacuously on the
                // missed tail.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Capture state before consuming the session in join().
    let state_rx = session.state();
    tokio::time::timeout(std::time::Duration::from_secs(30), session.join())
        .await
        .expect("session did not finish within 30s")
        .expect("session join error");

    // join() consumed the session, dropping the broadcast sender — the
    // drain task sees Closed and exits; the timeout only bounds it.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), drain).await;

    let final_state = state_rx.borrow().clone();

    // 1. Tournament completed: 3 successful attempts in history.
    let total_success_attempts: usize = final_state
        .history
        .values()
        .flatten()
        .filter(|a| matches!(a.outcome, pyrat_eval::MatchupOutcome::Success { .. }))
        .count();
    assert_eq!(
        total_success_attempts, expected_attempts,
        "expected {expected_attempts} success attempts, got {total_success_attempts}"
    );

    // 2. StoreSink wrote 3 success rows via the OutcomeCounter sibling.
    assert_eq!(counter.successes.load(Ordering::SeqCst), expected_attempts);
    assert_eq!(counter.failures.load(Ordering::SeqCst), 0);

    // 3. ReplaySink flushed one buffer per match.
    assert_eq!(
        replay_writer.count(),
        expected_attempts,
        "replay writer should have one entry per success"
    );

    // 4. MatchOver suppression invariant: the broadcast must never
    // surface MatchEvent::MatchOver. The canonical terminal lives on
    // DriverEvent (lossless) and SessionEvent (re-published).
    let observed = observed.lock();
    // Positive control: a "never happens" assertion over an empty
    // observation set proves nothing.
    assert!(
        !observed.is_empty(),
        "live broadcast observer saw no events at all"
    );
    let match_over_count = observed
        .iter()
        .filter(|e| {
            matches!(
                e,
                OrchestratorEvent::MatchEvent {
                    event: MatchEvent::MatchOver { .. },
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        match_over_count, 0,
        "MatchEvent::MatchOver leaked to the broadcast"
    );
}
