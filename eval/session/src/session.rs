//! `EvalSession` — drives the planner-orchestrator-store loop.
//!
//! Responsibilities:
//! - Own the lossless `DriverEvent` mpsc from the orchestrator. Lifecycle
//!   events are the canonical mutator of `TournamentState`.
//! - Re-publish lifecycle to its own broadcast (`SessionEvent`) so consumers
//!   subscribing to the session see exactly the events that changed state.
//! - Pass through the orchestrator's broadcast (`OrchestratorEvent`) for
//!   live per-turn UIs.
//! - Atomic `subscribe()`: state-then-tail consistency under a publish
//!   mutex (sync only, no awaits inside).
//!
//! Two construction modes: `New` (create a tournament) and `Resume`
//! (reconstruct from store rows). Both produce a session that runs to
//! completion (planner.is_done && orchestrator.idle).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use pyrat::game::builder::GameConfig;
use pyrat_eval_store::{
    AddTournamentPlayerError, CreateTournamentError, EloOptions, EvalError, EvalStore,
    NewTournament, RegisterPlayerError, TournamentId, TournamentParticipant, TournamentRecord,
};
use pyrat_orchestrator::{
    CompositeSink, DriverEvent, FailureReason, MatchSink, Orchestrator, OrchestratorConfig,
    OrchestratorError, OrchestratorEvent, SinkRole,
};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::descriptor::EvalMatchDescriptor;
use crate::mapping::MappingError;
use crate::observation::Observation;
use crate::plan::{Planner, ResolvedPlayer};
use crate::state::{MatchupKey, TournamentState};
use crate::store_sink::StoreSink;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Specification for a brand-new tournament. `format`, `target_games_per_matchup`,
/// and `params_json` are stored opaquely; planners deserialize whatever they
/// need from `params_json`. `game_config` and `tournament_seed` are the
/// tournament's runtime identity. Bootstrap derives the durable record from
/// the config (via `mapping::game_config_to_record`) and stores both on the
/// tournament row so resume can validate the planner.
///
/// `Debug` and `Clone` are not derived: `GameConfig` itself is `Clone` but
/// not `Debug` — callers that need to log a spec should format the relevant
/// fields explicitly.
#[derive(Clone)]
pub struct TournamentSpec {
    pub format: String,
    pub target_games_per_matchup: Option<u32>,
    pub params_json: String,
    /// Runtime config — caller hands one source of truth. The bootstrap
    /// derives the `GameConfigRecord`, ensures the row, and returns the id.
    pub game_config: GameConfig,
    /// Tournament-level seed for `matchup_seed` derivation. Must be
    /// `<= i64::MAX` — SQLite's INTEGER column is signed, and the store
    /// rejects out-of-range values with
    /// `CreateTournamentError::SeedOutOfRange` so the caller's seed and
    /// the stored seed always agree.
    pub tournament_seed: u64,
}

/// Result of [`EvalSession::create_tournament`]. Carries both the freshly
/// allocated `TournamentId` and the content-hashed `game_config_id` resolved
/// from `TournamentSpec.game_config`. Callers plug the id into their planner
/// config; the `game_config_id` saves them a second `ensure_game_config`
/// round-trip.
#[derive(Debug, Clone)]
pub struct CreatedTournament {
    pub tournament_id: TournamentId,
    pub game_config_id: String,
}

/// Session-level tunables. Defaults are sensible for most callers.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Per-matchup ceiling on consecutive `FailureReason::SinkFlushError`
    /// terminals. When hit, the run loop aborts with
    /// [`SessionError::PersistentSinkFlushError`].
    ///
    /// Why only `SinkFlushError`: `durable_record=false` also covers
    /// `Cancelled` (intentional shutdown) and `Internal(_)` (ambiguous);
    /// neither belongs in an infinite-loop guard. `SinkFlushError` is the
    /// "infrastructure broken" signal — read-only DB, disk full, FK
    /// violation, etc. The counter resets on any other terminal for the
    /// same key (a successful match, durable failure, or cancellation).
    ///
    /// The counter is transient (in-memory, not persisted), so a fresh
    /// process retries from zero. Default: 5.
    pub max_consecutive_sink_flush_failures: u32,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_consecutive_sink_flush_failures: 5,
        }
    }
}

/// Reconstruct mode handed to `EvalSession::start`.
///
/// The session always runs against an *existing* tournament id. To create
/// one, call [`EvalSession::create_tournament`] first — it returns a
/// `TournamentId` you then plug into the planner config and pass here.
/// Two-phase split sidesteps the chicken-and-egg between "session creates
/// the tid" and "planner needs the tid before construction".
pub struct SessionMode {
    pub tournament_id: TournamentId,
}

/// Lifecycle events surfaced by the session. Translated from the
/// orchestrator's `DriverEvent` plus an extra `TournamentFinished` terminal
/// after the run-loop exits.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SessionEvent {
    MatchSubmitted {
        descriptor: EvalMatchDescriptor,
    },
    MatchStarted {
        descriptor: EvalMatchDescriptor,
    },
    MatchFinished {
        descriptor: EvalMatchDescriptor,
    },
    MatchFailed {
        descriptor: EvalMatchDescriptor,
        durable_record: bool,
    },
    TournamentFinished,
    /// Tournament terminated abnormally. Emitted in addition to the
    /// `SessionError` that `join`/`shutdown` return — consumers that only
    /// watch the broadcast (e.g. GUIs) get a signal that the tournament
    /// won't progress further. Currently the only producer is the
    /// persistent-sink-flush guard.
    TournamentAborted {
        reason: String,
    },
}

impl SessionEvent {
    fn from_driver(event: &DriverEvent<EvalMatchDescriptor>) -> Option<Self> {
        match event {
            DriverEvent::MatchQueued { descriptor, .. } => Some(SessionEvent::MatchSubmitted {
                descriptor: descriptor.clone(),
            }),
            DriverEvent::MatchStarted { descriptor, .. } => Some(SessionEvent::MatchStarted {
                descriptor: descriptor.clone(),
            }),
            DriverEvent::MatchFinished { outcome } => Some(SessionEvent::MatchFinished {
                descriptor: outcome.descriptor.clone(),
            }),
            DriverEvent::MatchFailed { failure } => Some(SessionEvent::MatchFailed {
                descriptor: failure.descriptor.clone(),
                durable_record: failure.durable_record,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("store error: {0}")]
    Store(#[from] EvalError),

    #[error("register player failed: {0}")]
    RegisterPlayer(#[from] RegisterPlayerError),

    #[error("attach tournament player failed: {0}")]
    AddTournamentPlayer(#[from] AddTournamentPlayerError),

    #[error("create tournament failed: {0}")]
    CreateTournament(#[from] CreateTournamentError),

    #[error("game-config mapping failed: {0}")]
    Mapping(#[from] MappingError),

    #[error("tournament {0:?} not found in store")]
    TournamentNotFound(TournamentId),

    #[error("orchestrator error: {0}")]
    Orchestrator(#[from] OrchestratorError),

    /// The session run loop panicked. The string is the panic message
    /// stringified — `tokio::task::JoinError` itself is not part of the
    /// public API so we don't carry it through.
    #[error("run loop panicked: {0}")]
    RunLoopPanicked(String),

    /// A `spawn_blocking` task panicked during setup (bootstrap or resume
    /// reconstruction). `what` identifies the task; `message` is the
    /// stringified panic. Distinct from `RunLoopPanicked` because the
    /// failure happens before the run loop is even spawned.
    #[error("background task `{what}` panicked: {message}")]
    TaskPanicked { what: &'static str, message: String },

    /// The planner handed to `EvalSession::start` doesn't match the stored
    /// tournament's spec (players, game config, seed, or per-pair target).
    /// Reconstructing state from someone else's tournament would silently
    /// fragment standings.
    #[error("planner does not match stored tournament: {0}")]
    TournamentMismatch(String),

    /// A single matchup has hit the configured ceiling of consecutive
    /// `SinkFlushError` terminals (see
    /// [`SessionConfig::max_consecutive_sink_flush_failures`]). The store
    /// is likely broken in a non-transient way (read-only DB, disk full,
    /// schema mismatch, ...); operator intervention is needed.
    #[error("persistent sink-flush failure for matchup {matchup_key:?} at attempt {attempt_index}: {last_message}")]
    PersistentSinkFlushError {
        matchup_key: MatchupKey,
        attempt_index: u32,
        last_message: String,
    },
}

// ---------------------------------------------------------------------------
// EvalSession
// ---------------------------------------------------------------------------

/// Session ties orchestrator + planner + store together for one tournament.
///
/// The session owns the driver mpsc, applies lifecycle events to its
/// `TournamentState`, and re-publishes them to its own broadcast for
/// consumers. Live per-turn events flow through `live_events()`
/// (pass-through to the orchestrator's broadcast).
pub struct EvalSession {
    state_watch: watch::Sender<TournamentState>,
    session_events: broadcast::Sender<SessionEvent>,
    publish_mutex: Arc<Mutex<()>>,
    /// `Option` so `shutdown` can `.take()` the Arc and `Arc::try_unwrap`
    /// it. Rust forbids partial moves out of types with a `Drop` impl,
    /// so the only way to keep `Drop for EvalSession` *and* hand `orch`
    /// to `try_unwrap` is to gate it behind `Option`.
    orch: Option<Arc<Orchestrator<EvalMatchDescriptor>>>,
    /// Cancellation seam for `shutdown` and `Drop`. The run-loop selects
    /// on this alongside `driver_rx.recv()` so cancellation always wins
    /// even if no further lifecycle events arrive.
    cancel: CancellationToken,
    /// Run-loop join handle. `None` after `shutdown`/`join` consume it.
    run_loop_handle: Option<JoinHandle<Result<(), SessionError>>>,
}

impl EvalSession {
    /// Create a fresh tournament: ensure the game config row, register
    /// players, insert the tournament, attach participants. Returns both
    /// the freshly allocated `TournamentId` and the resolved
    /// `game_config_id` (caller plugs both into their planner config
    /// before [`Self::start`]).
    pub async fn create_tournament(
        store: Arc<Mutex<EvalStore>>,
        spec: TournamentSpec,
        players: Vec<ResolvedPlayer>,
    ) -> Result<CreatedTournament, SessionError> {
        bootstrap_new_tournament(store, &spec, &players).await
    }

    /// Construct + start the session against an existing tournament id.
    /// Spawns the run-loop task; `shutdown` awaits it cleanly.
    ///
    /// `elo_options` controls Elo recompute on each `MatchFinished`. The
    /// anchor player must be one of the resolved player ids.
    /// `session_config` carries session-level tunables (defaults are fine
    /// for most callers).
    ///
    /// Validates the planner against the stored tournament's spec
    /// (tournament_id, players, game_config_id, tournament_seed,
    /// target_games_per_matchup). Returns `SessionError::TournamentMismatch`
    /// on any divergence so a drifted planner can't silently fragment the
    /// tournament's history.
    pub async fn start<P: Planner + 'static>(
        store: Arc<Mutex<EvalStore>>,
        mode: SessionMode,
        planner: P,
        orch_config: OrchestratorConfig,
        elo_options: EloOptions,
        session_config: SessionConfig,
    ) -> Result<Self, SessionError> {
        let (tournament_record, tournament_players, initial_state) =
            reconstruct_tournament_state(store.clone(), mode.tournament_id).await?;

        validate_planner_against_stored_spec(&planner, &tournament_record, &tournament_players)?;

        Self::launch(
            store,
            initial_state,
            planner,
            orch_config,
            elo_options,
            session_config,
        )
    }

    /// Atomic `(state, events)` snapshot+tail. Lifecycle effects reflected
    /// in the snapshot precede any subsequent broadcast item the receiver
    /// observes — same contract as the orchestrator's `subscribe`.
    pub fn subscribe(&self) -> (TournamentState, broadcast::Receiver<SessionEvent>) {
        let _g = self.publish_mutex.lock();
        let state = self.state_watch.borrow().clone();
        let rx = self.session_events.subscribe();
        (state, rx)
    }

    /// Watch over `TournamentState`. Useful for one-shot snapshots.
    pub fn state(&self) -> watch::Receiver<TournamentState> {
        self.state_watch.subscribe()
    }

    /// Lossy session-lifecycle event stream. Use [`Self::subscribe`] for
    /// snapshot+tail consistency.
    pub fn events(&self) -> broadcast::Receiver<SessionEvent> {
        self.session_events.subscribe()
    }

    /// Pass-through to the orchestrator's broadcast: every event including
    /// per-turn `MatchEvent`s. Lossy. Used by GUIs that want live per-turn
    /// detail.
    ///
    /// # Panics
    /// Panics if called after [`Self::shutdown`] (the orchestrator handle
    /// is consumed there). A live session always has it.
    pub fn live_events(&self) -> broadcast::Receiver<OrchestratorEvent<EvalMatchDescriptor>> {
        self.orch
            .as_ref()
            .expect("live_events called after shutdown")
            .events()
    }

    /// Wait for the tournament to finish (planner.is_done && orch.idle).
    /// Returns when the run-loop emits `TournamentFinished`. Surfaces
    /// `SessionError::RunLoopPanicked` if the run-loop task panicked.
    pub async fn join(mut self) -> Result<(), SessionError> {
        if let Some(h) = self.run_loop_handle.take() {
            await_run_loop(h).await?;
        }
        Ok(())
    }

    /// Cancel the run loop and drain the orchestrator.
    ///
    /// Sequence:
    /// 1. Cancel the session token — the run-loop's `select!` picks this up
    ///    and exits on the next iteration, dropping its `Arc<Orchestrator>`
    ///    clone.
    /// 2. Await the run-loop handle.
    /// 3. Reclaim the orchestrator via `Arc::try_unwrap` and call its
    ///    consuming `shutdown().await` for graceful drain. Falls back to
    ///    `abort()` if any other `Arc` clone exists.
    ///
    /// **Drain guarantee.** Graceful drain runs only when `EvalSession`
    /// owns the last `Arc<Orchestrator>` at shutdown time. Today only the
    /// session struct and the run-loop task clone the Arc; the loop drops
    /// its clone when it exits, so `try_unwrap` succeeds and drain runs.
    /// If a future caller retains a clone (for cross-session inspection,
    /// say), `shutdown` falls back to `abort` and drain becomes best-effort.
    pub async fn shutdown(mut self) -> Result<(), SessionError> {
        self.cancel.cancel();
        if let Some(h) = self.run_loop_handle.take() {
            await_run_loop(h).await?;
        }
        if let Some(orch) = self.orch.take() {
            match Arc::try_unwrap(orch) {
                Ok(o) => o.shutdown().await,
                Err(a) => a.abort(),
            }
        }
        Ok(())
    }
}

impl Drop for EvalSession {
    /// Cancel the run loop if the session is dropped without `shutdown`
    /// or `join`. The loop's `select!` picks up the cancellation on its
    /// next iteration and exits, dropping its `Arc<Orchestrator>` clone;
    /// the session's own clone goes away with `self`. The orchestrator's
    /// tasks then drain through its own `Drop`.
    ///
    /// Fire-and-forget: we can't `await` here, so this is best-effort.
    /// Callers who need a guaranteed graceful drain should call
    /// [`Self::shutdown`].
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Map a finished run-loop `JoinHandle` to a `SessionError`-shaped result.
/// Panics flow through as `RunLoopPanicked`; the inner `Result` carries any
/// `SessionError` the run-loop returned.
async fn await_run_loop(h: JoinHandle<Result<(), SessionError>>) -> Result<(), SessionError> {
    match h.await {
        Ok(inner) => inner,
        Err(join_err) => Err(SessionError::RunLoopPanicked(join_err.to_string())),
    }
}

impl EvalSession {
    fn launch<P: Planner + 'static>(
        store: Arc<Mutex<EvalStore>>,
        initial_state: TournamentState,
        planner: P,
        orch_config: OrchestratorConfig,
        elo_options: EloOptions,
        session_config: SessionConfig,
    ) -> Result<Self, SessionError> {
        let store_sink: Arc<dyn MatchSink<EvalMatchDescriptor>> =
            Arc::new(StoreSink::new(store.clone()));
        let sinks = vec![(SinkRole::Required, store_sink)];
        Self::launch_with_sinks(
            initial_state,
            planner,
            orch_config,
            elo_options,
            session_config,
            sinks,
        )
    }

    /// Test seam: build the orchestrator from caller-supplied sinks instead
    /// of the default `StoreSink`. Lets unit tests plant a deliberately
    /// failing sink to exercise [`SessionConfig::max_consecutive_sink_flush_failures`].
    /// Not part of the public API. The tournament id lives on
    /// `initial_state.tournament_id`, so the explicit parameter that
    /// earlier versions carried was redundant.
    pub(crate) fn launch_with_sinks<P: Planner + 'static>(
        mut initial_state: TournamentState,
        planner: P,
        orch_config: OrchestratorConfig,
        elo_options: EloOptions,
        session_config: SessionConfig,
        sinks: Vec<(SinkRole, Arc<dyn MatchSink<EvalMatchDescriptor>>)>,
    ) -> Result<Self, SessionError> {
        let composite = Arc::new(CompositeSink::new(sinks));
        let (orch, driver_rx) = Orchestrator::<EvalMatchDescriptor>::new(orch_config, composite);
        let orch = Arc::new(orch);

        // Recompute Elo on the initial state *before* publishing to the
        // watch. Otherwise a subscriber hitting `subscribe()` between
        // `start()` returning and the run-loop's first iteration sees a
        // resumed-tournament snapshot with empty standings.
        initial_state.recompute_elo(&elo_options);

        let (state_watch, _state_rx) = watch::channel(initial_state.clone());
        let (session_events, _events_rx) = broadcast::channel(256);
        let publish_mutex = Arc::new(Mutex::new(()));
        let cancel = CancellationToken::new();

        let run_loop_handle = tokio::spawn(run_loop(
            planner,
            initial_state,
            state_watch.clone(),
            session_events.clone(),
            publish_mutex.clone(),
            orch.clone(),
            driver_rx,
            elo_options,
            session_config,
            cancel.clone(),
        ));

        Ok(Self {
            state_watch,
            session_events,
            publish_mutex,
            orch: Some(orch),
            cancel,
            run_loop_handle: Some(run_loop_handle),
        })
    }

    /// Test seam: clone of the session's cancellation token. Used by the
    /// Drop test to verify that dropping the session fires cancellation.
    #[cfg(test)]
    pub(crate) fn cancel_token_for_test(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Test seam: weak reference to the orchestrator. Used by the Drop
    /// test to verify the run loop actually drops its `Arc` clone after
    /// cancellation, proving the leak is closed.
    #[cfg(test)]
    pub(crate) fn orch_weak_for_test(&self) -> std::sync::Weak<Orchestrator<EvalMatchDescriptor>> {
        Arc::downgrade(
            self.orch
                .as_ref()
                .expect("orch_weak_for_test called after shutdown"),
        )
    }
}

// ---------------------------------------------------------------------------
// Construction modes
// ---------------------------------------------------------------------------

/// Bootstrap a new tournament: derive the durable `GameConfigRecord` from
/// the runtime `GameConfig`, ensure the row, register identity-bearing
/// player rows, insert the tournament with `game_config_id` +
/// `tournament_seed`, attach participants. Returns the resolved id pair.
///
/// `ensure_game_config` is the single source of truth for `game_config_id`:
/// content-hashed, idempotent. The caller never passes the id directly,
/// which removes any chance of disagreement between the durable form and
/// the runtime form.
async fn bootstrap_new_tournament(
    store: Arc<Mutex<EvalStore>>,
    spec: &TournamentSpec,
    players: &[ResolvedPlayer],
) -> Result<CreatedTournament, SessionError> {
    let game_config_record = crate::mapping::game_config_to_record(&spec.game_config)?;
    let format = spec.format.clone();
    let target_games_per_matchup = spec.target_games_per_matchup;
    let params_json = spec.params_json.clone();
    let tournament_seed = spec.tournament_seed;
    let players_to_register: Vec<_> = players
        .iter()
        .map(|p| pyrat_eval_store::NewPlayer {
            id: p.id.clone(),
            display_name: p.id.clone(),
            agent_id: player_agent_id(&p.spec).map(String::from),
            version: None,
            command: None,
            metadata_json: None,
        })
        .collect();

    tokio::task::spawn_blocking(move || {
        let mut store = store.lock();
        // Wrap the four bootstrap operations in one transaction so a
        // mid-sequence failure (e.g. a slot-taken or duplicate-player
        // error in `add_tournament_player`) doesn't leave a tournament
        // row + partial participants behind.
        store.transaction(move |tx| -> Result<CreatedTournament, SessionError> {
            let game_config_id = tx.ensure_game_config(&game_config_record)?;
            for p in &players_to_register {
                tx.register_player(p)?;
            }
            let new_tournament = NewTournament {
                format,
                target_games_per_matchup,
                params_json,
                game_config_id: game_config_id.clone(),
                tournament_seed,
            };
            let tid = tx.create_tournament(&new_tournament)?;
            for (slot, p) in players_to_register.iter().enumerate() {
                tx.add_tournament_player(tid, &p.id, slot as i64)?;
            }
            Ok(CreatedTournament {
                tournament_id: tid,
                game_config_id,
            })
        })
    })
    .await
    .map_err(|e| SessionError::TaskPanicked {
        what: "bootstrap_new_tournament",
        message: e.to_string(),
    })?
}

/// Reconstruct state from store rows for a resume.
///
/// Loads the tournament record, its participants, and all attempts in one
/// blocking call. The caller cross-checks the planner against the
/// returned record before launching.
async fn reconstruct_tournament_state(
    store: Arc<Mutex<EvalStore>>,
    tournament_id: TournamentId,
) -> Result<
    (
        TournamentRecord,
        Vec<TournamentParticipant>,
        TournamentState,
    ),
    SessionError,
> {
    tokio::task::spawn_blocking(move || {
        let store = store.lock();
        let tournament = store
            .get_tournament(tournament_id)?
            .ok_or(SessionError::TournamentNotFound(tournament_id))?;
        let tournament_players = store.get_tournament_players(tournament.id)?;
        let mut state = TournamentState::empty(tournament.id);
        for attempt in store.get_attempts(tournament.id, None)? {
            state.fold_attempt(&attempt);
        }
        Ok::<_, SessionError>((tournament, tournament_players, state))
    })
    .await
    .map_err(|e| SessionError::TaskPanicked {
        what: "reconstruct_tournament_state",
        message: e.to_string(),
    })?
}

/// Cross-check the planner's expected spec against what the store has on
/// the tournament. Anything that diverges returns
/// `SessionError::TournamentMismatch` with a precise reason.
fn validate_planner_against_stored_spec(
    planner: &impl Planner,
    tournament: &TournamentRecord,
    tournament_players: &[TournamentParticipant],
) -> Result<(), SessionError> {
    if planner.expected_format() != tournament.format {
        return Err(SessionError::TournamentMismatch(format!(
            "format: planner expected {:?}, stored is {:?}",
            planner.expected_format(),
            tournament.format
        )));
    }
    if planner.tournament_id() != tournament.id {
        return Err(SessionError::TournamentMismatch(format!(
            "tournament_id: planner expected {:?}, stored is {:?}",
            planner.tournament_id(),
            tournament.id
        )));
    }
    if planner.expected_game_config_id() != tournament.game_config_id {
        return Err(SessionError::TournamentMismatch(format!(
            "game_config_id: planner expected {:?}, stored is {:?}",
            planner.expected_game_config_id(),
            tournament.game_config_id
        )));
    }
    if planner.expected_tournament_seed() != tournament.tournament_seed {
        return Err(SessionError::TournamentMismatch(format!(
            "tournament_seed: planner expected {}, stored is {}",
            planner.expected_tournament_seed(),
            tournament.tournament_seed
        )));
    }
    // target_games_per_matchup: both sides know it only when both surface
    // a value. Don't enforce when either side is None.
    if let (Some(planner_target), Some(stored_target)) = (
        planner.expected_target_per_pair(),
        tournament.target_games_per_matchup,
    ) {
        if planner_target != stored_target {
            return Err(SessionError::TournamentMismatch(format!(
                "target_games_per_matchup: planner expected {planner_target}, stored is {stored_target}",
            )));
        }
    }
    // Players: stored rows are sorted by slot (`get_tournament_players`'s
    // ORDER BY). Compare by id+slot in that order.
    let stored_ids: Vec<&str> = tournament_players
        .iter()
        .map(|p| p.player_id.as_str())
        .collect();
    let expected_ids = planner.expected_players();
    if expected_ids != stored_ids {
        return Err(SessionError::TournamentMismatch(format!(
            "players: planner expected {expected_ids:?}, stored is {stored_ids:?}",
        )));
    }
    Ok(())
}

fn player_agent_id(spec: &pyrat_orchestrator::PlayerSpec) -> Option<&str> {
    match spec {
        pyrat_orchestrator::PlayerSpec::Subprocess { agent_id, .. }
        | pyrat_orchestrator::PlayerSpec::Embedded { agent_id, .. } => Some(agent_id),
        // PlayerSpec is `#[non_exhaustive]`. Return None for unknown
        // variants so the row registers with `agent_id = NULL`; the
        // `register_player` NULL-fill path then applies on later updates.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Run loop
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn run_loop<P: Planner>(
    mut planner: P,
    mut state: TournamentState,
    state_watch: watch::Sender<TournamentState>,
    session_events: broadcast::Sender<SessionEvent>,
    publish_mutex: Arc<Mutex<()>>,
    orch: Arc<Orchestrator<EvalMatchDescriptor>>,
    mut driver_rx: mpsc::Receiver<DriverEvent<EvalMatchDescriptor>>,
    elo_options: EloOptions,
    session_config: SessionConfig,
    cancel: CancellationToken,
) -> Result<(), SessionError> {
    // Initial Elo was recomputed in `launch_with_sinks` before this task
    // was spawned, so a subscriber that lands during start sees standings
    // immediately. The watch was already populated with that state.

    // Transient (in-memory) counter of consecutive `SinkFlushError` terminals
    // per matchup key. See `SessionConfig::max_consecutive_sink_flush_failures`.
    let mut sink_flush_counts: HashMap<MatchupKey, u32> = HashMap::new();

    loop {
        let cap = orch.available_capacity();
        if cap > 0 {
            let batch = {
                let orch_for_alloc = orch.clone();
                let mut allocate = move || orch_for_alloc.allocate_id();
                planner.next_batch(&state, cap, &mut allocate)
            };
            for matchup in batch {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Ok(()),
                    res = orch.submit(matchup) => {
                        if res.is_err() {
                            // Orchestrator shut down underneath us.
                            return Ok(());
                        }
                    }
                }
            }
        }

        if planner.is_done(&state) && orch.idle() {
            let _ = session_events.send(SessionEvent::TournamentFinished);
            return Ok(());
        }

        let event = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok(()),
            maybe = driver_rx.recv() => match maybe {
                Some(e) => e,
                // Driver channel closed (orchestrator dropped). Tournament
                // can't progress further.
                None => return Ok(()),
            }
        };
        state.apply(&event);
        if matches!(event, DriverEvent::MatchFinished { .. }) {
            state.recompute_elo(&elo_options);
        }
        {
            let _g = publish_mutex.lock();
            state_watch.send_replace(state.clone());
            if let Some(se) = SessionEvent::from_driver(&event) {
                let _ = session_events.send(se);
            }
        }
        if let Some(obs) = Observation::from_driver_event(&event) {
            planner.on_observation(&obs);
        }

        // Persistent-sink-flush guard. Counts only `SinkFlushError` so
        // benign non-durable terminals (`Cancelled`, `Internal`) don't
        // burn the counter. Any other terminal for the same key resets it.
        if let Some(check) = persistent_failure_check(
            &event,
            &mut sink_flush_counts,
            session_config.max_consecutive_sink_flush_failures,
        ) {
            let _ = session_events.send(SessionEvent::TournamentAborted {
                reason: format!(
                    "persistent sink-flush failure: {} consecutive `SinkFlushError`s for matchup {:?} (last: {})",
                    session_config.max_consecutive_sink_flush_failures,
                    check.matchup_key,
                    check.last_message
                ),
            });
            return Err(SessionError::PersistentSinkFlushError {
                matchup_key: check.matchup_key,
                attempt_index: check.attempt_index,
                last_message: check.last_message,
            });
        }
    }
}

/// Per-event update of the per-matchup sink-flush counter. Returns the
/// breach details when a matchup hits `threshold` consecutive
/// `SinkFlushError`s, otherwise `None`.
fn persistent_failure_check(
    event: &DriverEvent<EvalMatchDescriptor>,
    counts: &mut HashMap<MatchupKey, u32>,
    threshold: u32,
) -> Option<PersistentFailureBreach> {
    let DriverEvent::MatchFailed { failure } = event else {
        // Non-terminal events don't update the counter.
        // MatchFinished terminals reset it (handled below via the
        // explicit MatchFinished arm).
        if let DriverEvent::MatchFinished { outcome } = event {
            counts.remove(&MatchupKey::from_descriptor(&outcome.descriptor));
        }
        return None;
    };
    let key = MatchupKey::from_descriptor(&failure.descriptor);
    let is_sink_flush = matches!(failure.reason, FailureReason::SinkFlushError(_));
    if !is_sink_flush {
        // Any other terminal — durable failure, cancellation, internal —
        // resets the counter for this matchup.
        counts.remove(&key);
        return None;
    }
    let count = counts.entry(key.clone()).or_insert(0);
    *count += 1;
    if *count >= threshold {
        let last_message = match &failure.reason {
            FailureReason::SinkFlushError(s) => s.clone(),
            _ => unreachable!("guard above guarantees SinkFlushError"),
        };
        Some(PersistentFailureBreach {
            matchup_key: key,
            attempt_index: failure.descriptor.attempt_index,
            last_message,
        })
    } else {
        None
    }
}

struct PersistentFailureBreach {
    matchup_key: MatchupKey,
    attempt_index: u32,
    last_message: String,
}

#[cfg(test)]
mod tests {
    //! Tests that exercise the `pub(crate)` test seam `launch_with_sinks`
    //! and the persistent-sink-flush guard. Lives in `src/` (not `tests/`)
    //! because integration tests compile as external crates and can't see
    //! `pub(crate)` items.

    use std::time::{Duration, SystemTime};

    use async_trait::async_trait;
    use pyrat_host::match_host::MatchEvent;
    use pyrat_host::player::PlayerIdentity;
    use pyrat_orchestrator::{MatchFailure, MatchId, MatchOutcome, OrchestratorConfig, SinkError};

    use super::*;

    /// Synthetic `MatchFailure` shaped for the persistent-failure tests.
    fn synthetic_failure(
        match_id: u64,
        attempt_index: u32,
        player1: &str,
        player2: &str,
        reason: FailureReason,
    ) -> MatchFailure<EvalMatchDescriptor> {
        MatchFailure {
            descriptor: EvalMatchDescriptor {
                match_id: MatchId(match_id),
                tournament_id: TournamentId(1),
                game_config_id: "gc".into(),
                player1_id: player1.into(),
                player2_id: player2.into(),
                seed: 7,
                repetition_index: 0,
                attempt_index,
                planned_at: SystemTime::UNIX_EPOCH,
            },
            started_at: None,
            failed_at: SystemTime::UNIX_EPOCH,
            reason,
            players: None,
            // `durable_record=false` is the value the run-loop sees from
            // the orchestrator after a sink flush failure.
            durable_record: false,
        }
    }

    fn driver_failed(
        failure: MatchFailure<EvalMatchDescriptor>,
    ) -> DriverEvent<EvalMatchDescriptor> {
        DriverEvent::MatchFailed { failure }
    }

    #[test]
    fn persistent_failure_check_increments_on_sink_flush() {
        let mut counts = HashMap::new();
        // 4 of 5 — not yet a breach.
        for i in 0..4 {
            let ev = driver_failed(synthetic_failure(
                i,
                i as u32,
                "a",
                "b",
                FailureReason::SinkFlushError(format!("planted #{i}")),
            ));
            assert!(persistent_failure_check(&ev, &mut counts, 5).is_none());
        }
        // 5th SinkFlushError trips the threshold.
        let ev = driver_failed(synthetic_failure(
            4,
            4,
            "a",
            "b",
            FailureReason::SinkFlushError("planted #4".into()),
        ));
        let breach = persistent_failure_check(&ev, &mut counts, 5).expect("threshold should fire");
        assert_eq!(breach.attempt_index, 4);
        assert_eq!(breach.last_message, "planted #4");
        assert_eq!(breach.matchup_key.player1_id(), "a");
        assert_eq!(breach.matchup_key.player2_id(), "b");
    }

    #[test]
    fn persistent_failure_check_resets_on_non_sink_flush_terminal() {
        let mut counts = HashMap::new();
        // Two SinkFlush failures.
        for i in 0..2 {
            persistent_failure_check(
                &driver_failed(synthetic_failure(
                    i,
                    i as u32,
                    "a",
                    "b",
                    FailureReason::SinkFlushError("planted".into()),
                )),
                &mut counts,
                5,
            );
        }
        assert_eq!(*counts.iter().next().unwrap().1, 2);

        // A `Cancelled` failure on the same matchup resets the counter.
        let cancel = driver_failed(synthetic_failure(2, 2, "a", "b", FailureReason::Cancelled));
        persistent_failure_check(&cancel, &mut counts, 5);
        assert!(counts.is_empty(), "Cancelled resets the counter");
    }

    #[test]
    fn persistent_failure_check_resets_on_match_finished() {
        let mut counts = HashMap::new();
        persistent_failure_check(
            &driver_failed(synthetic_failure(
                0,
                0,
                "a",
                "b",
                FailureReason::SinkFlushError("planted".into()),
            )),
            &mut counts,
            5,
        );
        assert_eq!(counts.len(), 1);

        // Synthesize a successful terminal for the same matchup.
        let success = DriverEvent::MatchFinished {
            outcome: MatchOutcome {
                descriptor: EvalMatchDescriptor {
                    match_id: MatchId(99),
                    tournament_id: TournamentId(1),
                    game_config_id: "gc".into(),
                    player1_id: "a".into(),
                    player2_id: "b".into(),
                    seed: 7,
                    repetition_index: 0,
                    attempt_index: 1,
                    planned_at: SystemTime::UNIX_EPOCH,
                },
                started_at: SystemTime::UNIX_EPOCH,
                finished_at: SystemTime::UNIX_EPOCH,
                result: pyrat_host::match_host::MatchResult {
                    result: pyrat_host::wire::GameResult::Draw,
                    player1_score: 0.0,
                    player2_score: 0.0,
                    turns_played: 5,
                },
                players: [
                    PlayerIdentity {
                        name: "a".into(),
                        author: "x".into(),
                        agent_id: "a".into(),
                        slot: pyrat_host::wire::Player::Player1,
                    },
                    PlayerIdentity {
                        name: "b".into(),
                        author: "x".into(),
                        agent_id: "b".into(),
                        slot: pyrat_host::wire::Player::Player2,
                    },
                ],
            },
        };
        persistent_failure_check(&success, &mut counts, 5);
        assert!(counts.is_empty(), "MatchFinished resets the counter");
    }

    /// A `MatchSink` that returns `Err` on every terminal callback,
    /// producing the `SinkFlushError` chain the guard is designed to catch.
    struct AlwaysFailingSink;

    #[async_trait]
    impl MatchSink<EvalMatchDescriptor> for AlwaysFailingSink {
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
            Err(SinkError {
                source: anyhow::anyhow!("planted sink failure"),
            })
        }
        async fn on_match_failed(
            &self,
            _failure: &MatchFailure<EvalMatchDescriptor>,
        ) -> Result<(), SinkError> {
            Ok(())
        }
    }

    /// End-to-end via `launch_with_sinks`: plant an always-failing sink and
    /// confirm the session aborts with `PersistentSinkFlushError` instead of
    /// looping forever.
    #[tokio::test]
    async fn launch_with_sinks_persists_sink_flush_failure_triggers_abort() {
        use crate::plan::{RoundRobinPlanner, RoundRobinPlannerConfig};
        use pyrat::game::builder::GameBuilder;
        use pyrat::{Coordinates, Direction};
        use pyrat_bot_api::Options;
        use pyrat_host::player::{EmbeddedBot, EmbeddedCtx};
        use pyrat_host::wire::TimingMode;
        use pyrat_orchestrator::{EmbeddedBotFactory, PlayerSpec, Timing};
        use pyrat_protocol::HashedTurnState;

        struct StayBot;
        impl Options for StayBot {}
        impl EmbeddedBot for StayBot {
            fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
                Direction::Stay
            }
        }

        let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));

        let game_config = GameBuilder::new(3, 3)
            .with_max_turns(5)
            .with_open_maze()
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
            .with_random_cheese(1, false)
            .build();

        let factory: EmbeddedBotFactory = Arc::new(|| Box::new(StayBot));
        let players = vec![
            ResolvedPlayer {
                id: "a".into(),
                spec: PlayerSpec::Embedded {
                    agent_id: "a".into(),
                    name: "a".into(),
                    author: "tests".into(),
                    factory: factory.clone(),
                },
            },
            ResolvedPlayer {
                id: "b".into(),
                spec: PlayerSpec::Embedded {
                    agent_id: "b".into(),
                    name: "b".into(),
                    author: "tests".into(),
                    factory,
                },
            },
        ];

        let spec = TournamentSpec {
            format: "round_robin".into(),
            target_games_per_matchup: Some(10),
            params_json: "{}".into(),
            game_config: game_config.clone(),
            tournament_seed: 0xC0FFEE,
        };
        let created = EvalSession::create_tournament(store.clone(), spec, players.clone())
            .await
            .expect("create_tournament");

        let planner = RoundRobinPlanner::new(RoundRobinPlannerConfig {
            players,
            game_config,
            game_config_id: created.game_config_id,
            timing: Timing {
                mode: TimingMode::Wait,
                move_timeout_ms: 1000,
                preprocessing_timeout_ms: 5000,
            },
            tournament_id: created.tournament_id,
            target_per_pair: 10,
            // High enough that the counter trips first, not max_failures.
            max_failures_per_pair: 999,
            tournament_seed: 0xC0FFEE,
        });

        let initial_state = TournamentState::empty(created.tournament_id);
        let failing_sink: Arc<dyn MatchSink<EvalMatchDescriptor>> = Arc::new(AlwaysFailingSink);
        let sinks = vec![(SinkRole::Required, failing_sink)];

        let session = EvalSession::launch_with_sinks(
            initial_state,
            planner,
            OrchestratorConfig {
                max_parallel: 1,
                ..OrchestratorConfig::default()
            },
            EloOptions::new("a"),
            SessionConfig {
                max_consecutive_sink_flush_failures: 3,
            },
            sinks,
        )
        .expect("launch");

        let result = tokio::time::timeout(Duration::from_secs(5), session.join())
            .await
            .expect("session should not hang");
        match result {
            Err(SessionError::PersistentSinkFlushError {
                attempt_index,
                last_message,
                ..
            }) => {
                // 3 consecutive failures → counter hits threshold on the 3rd,
                // so attempt_index in [0, 2].
                assert!(
                    attempt_index < 3,
                    "unexpected attempt_index {attempt_index}"
                );
                assert!(
                    last_message.contains("planted sink failure"),
                    "unexpected message: {last_message}"
                );
            },
            other => panic!("expected PersistentSinkFlushError, got {other:?}"),
        }
    }

    /// Drop without `join`/`shutdown` must cancel the run loop and let
    /// the orchestrator be released. Two assertions:
    ///   1. The cancel token transitions to cancelled — proves Drop fired.
    ///   2. The `Weak<Orchestrator>` fails to upgrade within a bounded
    ///      window — proves the run loop's `Arc` clone was actually
    ///      dropped (the leak is closed). Cancellation alone proves the
    ///      signal; the weak-release check proves the consequence.
    #[tokio::test]
    async fn dropping_session_cancels_run_loop_and_releases_orchestrator() {
        use crate::plan::{RoundRobinPlanner, RoundRobinPlannerConfig};
        use pyrat::game::builder::GameBuilder;
        use pyrat::{Coordinates, Direction};
        use pyrat_bot_api::Options;
        use pyrat_host::player::{EmbeddedBot, EmbeddedCtx};
        use pyrat_host::wire::TimingMode;
        use pyrat_orchestrator::{EmbeddedBotFactory, PlayerSpec, Timing};
        use pyrat_protocol::HashedTurnState;

        struct StayBot;
        impl Options for StayBot {}
        impl EmbeddedBot for StayBot {
            fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
                Direction::Stay
            }
        }

        let game_config = GameBuilder::new(3, 3)
            .with_max_turns(5)
            .with_open_maze()
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
            .with_random_cheese(1, false)
            .build();

        let factory: EmbeddedBotFactory = Arc::new(|| Box::new(StayBot));
        let players = vec![
            ResolvedPlayer {
                id: "a".into(),
                spec: PlayerSpec::Embedded {
                    agent_id: "a".into(),
                    name: "a".into(),
                    author: "tests".into(),
                    factory: factory.clone(),
                },
            },
            ResolvedPlayer {
                id: "b".into(),
                spec: PlayerSpec::Embedded {
                    agent_id: "b".into(),
                    name: "b".into(),
                    author: "tests".into(),
                    factory,
                },
            },
        ];

        // 100 games-per-pair: more than enough pending work so the loop
        // is busy when we drop. We never observe a terminal — Drop fires
        // mid-run.
        let planner = RoundRobinPlanner::new(RoundRobinPlannerConfig {
            players,
            game_config,
            game_config_id: "test-config".into(),
            timing: Timing {
                mode: TimingMode::Wait,
                move_timeout_ms: 1000,
                preprocessing_timeout_ms: 5000,
            },
            tournament_id: TournamentId(1),
            target_per_pair: 100,
            max_failures_per_pair: 999,
            tournament_seed: 0xC0FFEE,
        });

        let initial_state = TournamentState::empty(TournamentId(1));
        let noop_sink: Arc<dyn MatchSink<EvalMatchDescriptor>> =
            Arc::new(pyrat_orchestrator::NoOpSink::<EvalMatchDescriptor>::new());
        let sinks = vec![(SinkRole::Required, noop_sink)];

        let session = EvalSession::launch_with_sinks(
            initial_state,
            planner,
            OrchestratorConfig {
                max_parallel: 1,
                ..OrchestratorConfig::default()
            },
            EloOptions::new("a"),
            SessionConfig::default(),
            sinks,
        )
        .expect("launch");

        let token = session.cancel_token_for_test();
        let weak = session.orch_weak_for_test();
        assert!(
            weak.upgrade().is_some(),
            "orchestrator should be live before drop"
        );

        drop(session);

        // 1. Cancellation fired — proves Drop ran.
        tokio::time::timeout(Duration::from_secs(1), token.cancelled())
            .await
            .expect("cancel token should fire within 1s of drop");

        // 2. The Arc was actually released — proves the leak is closed.
        let start = std::time::Instant::now();
        let mut released = false;
        while start.elapsed() < Duration::from_secs(1) {
            if weak.upgrade().is_none() {
                released = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            released,
            "orchestrator Arc was not released within 1s of drop"
        );
    }
}
