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

use std::sync::Arc;

use parking_lot::Mutex;
use pyrat::game::builder::GameConfig;
use pyrat_eval_store::{
    AddTournamentPlayerError, CreateTournamentError, EloOptions, EvalError, EvalStore,
    NewTournament, RegisterPlayerError, TournamentId,
};
use pyrat_orchestrator::{
    CompositeSink, DriverEvent, MatchSink, Orchestrator, OrchestratorConfig, OrchestratorError,
    OrchestratorEvent, SinkRole,
};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;

use crate::descriptor::EvalMatchDescriptor;
use crate::mapping::MappingError;
use crate::observation::Observation;
use crate::plan::{Planner, ResolvedPlayer};
use crate::state::TournamentState;
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
    /// Tournament-level seed for `matchup_seed` derivation. The high bit is
    /// dropped at the store boundary; callers can pass any `u64`.
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
    orch: Arc<Orchestrator<EvalMatchDescriptor>>,
    /// Held so `live_events()` can hand out new receivers via `orch.events()`.
    /// (Field exists implicitly through `orch` — no extra storage.)
    /// Run-loop join handle. `None` after `shutdown` consumes the session.
    run_loop_handle: Option<JoinHandle<()>>,
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
    pub async fn start<P: Planner + 'static>(
        store: Arc<Mutex<EvalStore>>,
        mode: SessionMode,
        planner: P,
        orch_config: OrchestratorConfig,
        elo_options: EloOptions,
    ) -> Result<Self, SessionError> {
        let (tournament_id, initial_state) =
            reconstruct_tournament_state(store.clone(), mode.tournament_id).await?;

        Self::launch(
            store,
            tournament_id,
            initial_state,
            planner,
            orch_config,
            elo_options,
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
    pub fn live_events(&self) -> broadcast::Receiver<OrchestratorEvent<EvalMatchDescriptor>> {
        self.orch.events()
    }

    /// Wait for the tournament to finish (planner.is_done && orch.idle).
    /// Returns when the run-loop emits `TournamentFinished`.
    pub async fn join(mut self) {
        if let Some(h) = self.run_loop_handle.take() {
            let _ = h.await;
        }
    }

    /// Cancel the run loop and shut down the orchestrator. Blocks until
    /// every match has terminated (with `MatchFailed { Cancelled }` for
    /// in-flight ones).
    ///
    /// The orchestrator's `Drop` impl handles its own task abort once the
    /// last `Arc` ref drops; we don't `try_unwrap` to call its consuming
    /// `shutdown(self)` because the run-loop holds an `Arc` clone that
    /// only drops after `h.await` returns — at that point `self` is also
    /// going out of scope, so the `Drop` path runs naturally.
    pub async fn shutdown(mut self) {
        self.orch.abort();
        if let Some(h) = self.run_loop_handle.take() {
            let _ = h.await;
        }
    }
}

impl EvalSession {
    fn launch<P: Planner + 'static>(
        store: Arc<Mutex<EvalStore>>,
        tournament_id: TournamentId,
        initial_state: TournamentState,
        planner: P,
        orch_config: OrchestratorConfig,
        elo_options: EloOptions,
    ) -> Result<Self, SessionError> {
        let store_sink: Arc<dyn MatchSink<EvalMatchDescriptor>> =
            Arc::new(StoreSink::new(store.clone()));
        let composite = Arc::new(CompositeSink::new(vec![(SinkRole::Required, store_sink)]));
        let (orch, driver_rx) = Orchestrator::<EvalMatchDescriptor>::new(orch_config, composite);
        let orch = Arc::new(orch);

        let (state_watch, _state_rx) = watch::channel(initial_state.clone());
        let (session_events, _events_rx) = broadcast::channel(256);
        let publish_mutex = Arc::new(Mutex::new(()));

        let run_loop_handle = tokio::spawn(run_loop(
            planner,
            initial_state,
            tournament_id,
            state_watch.clone(),
            session_events.clone(),
            publish_mutex.clone(),
            orch.clone(),
            driver_rx,
            elo_options,
        ));

        Ok(Self {
            state_watch,
            session_events,
            publish_mutex,
            orch,
            run_loop_handle: Some(run_loop_handle),
        })
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
            agent_id: Some(player_agent_id(&p.spec).into()),
            version: None,
            command: None,
            metadata_json: None,
        })
        .collect();

    tokio::task::spawn_blocking(move || {
        let store = store.lock();
        let game_config_id = store.ensure_game_config(&game_config_record)?;
        for p in &players_to_register {
            store.register_player(p)?;
        }
        let new_tournament = NewTournament {
            format,
            target_games_per_matchup,
            params_json,
            game_config_id: game_config_id.clone(),
            tournament_seed,
        };
        let tid = store.create_tournament(&new_tournament)?;
        for (slot, p) in players_to_register.iter().enumerate() {
            store.add_tournament_player(tid, &p.id, slot as i64)?;
        }
        Ok::<CreatedTournament, SessionError>(CreatedTournament {
            tournament_id: tid,
            game_config_id,
        })
    })
    .await
    .expect("bootstrap blocking task panicked")
}

/// Reconstruct state from store rows for a resume.
///
/// Loads attempts once (no per-loop queries). Folds successes into Elo
/// and into per-MatchupKey history. The planner picks up from there: it
/// re-issues missing slots (no row yet) and retries failed slots up to
/// `max_failures_per_pair` (its own config).
async fn reconstruct_tournament_state(
    store: Arc<Mutex<EvalStore>>,
    tournament_id: TournamentId,
) -> Result<(TournamentId, TournamentState), SessionError> {
    let (tournament_id, state) = tokio::task::spawn_blocking(move || {
        let store = store.lock();
        let tournament = store
            .get_tournament(tournament_id)?
            .ok_or(SessionError::TournamentNotFound(tournament_id))?;
        let mut state = TournamentState::empty(tournament.id);
        for attempt in store.get_attempts(tournament.id, None)? {
            state.fold_attempt(&attempt);
        }
        Ok::<(TournamentId, TournamentState), SessionError>((tournament.id, state))
    })
    .await
    .expect("reconstruct blocking task panicked")?;
    Ok((tournament_id, state))
}

fn player_agent_id(spec: &pyrat_orchestrator::PlayerSpec) -> &str {
    match spec {
        pyrat_orchestrator::PlayerSpec::Subprocess { agent_id, .. } => agent_id,
        pyrat_orchestrator::PlayerSpec::Embedded { agent_id, .. } => agent_id,
        // PlayerSpec is `#[non_exhaustive]`. Falls back to "" for unknown
        // variants — the row gets registered without an agent_id, and the
        // ensure_player NULL-fill path applies on later updates.
        _ => "",
    }
}

// ---------------------------------------------------------------------------
// Run loop
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn run_loop<P: Planner>(
    mut planner: P,
    mut state: TournamentState,
    _tournament_id: TournamentId,
    state_watch: watch::Sender<TournamentState>,
    session_events: broadcast::Sender<SessionEvent>,
    publish_mutex: Arc<Mutex<()>>,
    orch: Arc<Orchestrator<EvalMatchDescriptor>>,
    mut driver_rx: mpsc::Receiver<DriverEvent<EvalMatchDescriptor>>,
    elo_options: EloOptions,
) {
    // Initial Elo on resume (so subscribers see standings before the first
    // MatchFinished arrives).
    state.recompute_elo(&elo_options);
    {
        let _g = publish_mutex.lock();
        state_watch.send_replace(state.clone());
    }

    loop {
        let cap = orch.available_capacity();
        if cap > 0 {
            let batch = {
                let orch_for_alloc = orch.clone();
                let mut allocate = move || orch_for_alloc.allocate_id();
                planner.next_batch(&state, cap, &mut allocate)
            };
            for matchup in batch {
                if orch.submit(matchup).await.is_err() {
                    // Orchestrator shut down underneath us.
                    return;
                }
            }
        }

        if planner.is_done(&state) && orch.idle() {
            let _ = session_events.send(SessionEvent::TournamentFinished);
            return;
        }

        match driver_rx.recv().await {
            Some(event) => {
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
            },
            None => {
                // Driver channel closed (orchestrator dropped). Tournament
                // can't progress further.
                return;
            },
        }
    }
}
