//! Concurrent match executor: the live half of `pyrat-orchestrator`.
//!
//! The executor reads `Matchup<D>` from a bounded submit channel, runs each
//! one through [`pyrat_host::match_host::Match`] on a `JoinSet`, and
//! publishes events down two channels:
//!
//! - **Lifecycle** (`MatchQueued` / `MatchStarted` / `MatchFinished` /
//!   `MatchFailed`) over a bounded `mpsc::Sender<DriverEvent<D>>`. Lossless,
//!   single-consumer; the owning driver (an `EvalSession`, the run-one
//!   driver, etc.) holds the receiver.
//! - **Live**, including per-turn `MatchEvent`s, over a `broadcast`. Lossy
//!   on slow consumers (the live UI tradeoff).
//!
//! State transitions are serialised through a `parking_lot::Mutex` over
//! [`ExecutorState`], the watch sender, and the broadcast send. The lock is
//! never held across an `.await`; the only awaited step before grabbing it
//! is `driver_tx.send`, which provides backpressure when the driver lags.
//!
//! Cancellation is RAII: each per-match task gets a child of the root
//! `CancellationToken`, watches it inside its `select!`, and drops the
//! `Match::run` future on cancel. `BotProcesses` (held inside that task)
//! reaps subprocesses through `Drop`.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use pyrat_host::match_host::{PlayingConfig, SetupTiming};
use tokio::sync::{broadcast, mpsc, watch, Semaphore};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use tracing::{error, warn};

use crate::descriptor::Descriptor;
use crate::error::{OrchestratorError, OrchestratorInternalError};
use crate::event::OrchestratorEvent;
use crate::id::MatchId;
use crate::matchup::Matchup;
use crate::run_match;
use crate::sink::MatchSink;
use crate::state::ExecutorState;

// Re-export the lifecycle DTO so callers see one path: `Orchestrator` ⇒
// `DriverEvent`.
pub use crate::event::DriverEvent;

/// Tunables for one [`Orchestrator`] instance.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Maximum number of matches running concurrently (semaphore capacity).
    pub max_parallel: usize,
    /// Bound on the `submit` mpsc. Backpressure surface for very fast
    /// planners.
    pub submit_capacity: usize,
    /// Bound on the lifecycle `mpsc<DriverEvent>`. Once full, the run-loop
    /// awaits on `send`: the natural backpressure for a slow driver.
    pub driver_events_capacity: usize,
    /// Bound on the live `broadcast`. Slow consumers see `Lagged(n)` and
    /// skip ahead (that's the design; live broadcast is lossy).
    pub broadcast_capacity: usize,
    /// Default host-side setup timing for every match.
    pub setup_timing: SetupTiming,
    /// Default host-side playing config for every match. Cloned per match
    /// because [`PlayingConfig`] is `Clone` and the host owns it as a
    /// per-`Match` value.
    pub playing_config: PlayingConfig,
    /// TCP `Identify → Welcome` handshake timeout passed to
    /// `accept_players`. Per-match, not per-orchestrator-lifetime.
    pub handshake_timeout: Duration,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_parallel: 1,
            submit_capacity: 64,
            driver_events_capacity: 64,
            broadcast_capacity: 256,
            setup_timing: SetupTiming::default(),
            playing_config: PlayingConfig::default(),
            handshake_timeout: Duration::from_secs(10),
        }
    }
}

/// Per-match runtime config threaded through `run_match`. Cheap to clone
/// (Arc'd at the spawn site).
#[derive(Debug)]
pub(crate) struct RunConfig {
    pub setup_timing: SetupTiming,
    pub playing_config: PlayingConfig,
    pub handshake_timeout: Duration,
}

/// Shared state used by both the run-loop and per-match tasks.
pub(crate) struct ExecutorInner<D: Descriptor> {
    /// Live state, also acting as the publish mutex. Held only across
    /// `apply_state_transition` + `state_tx.send_replace` + `broadcast.send`,
    /// none of which `.await`.
    state: Mutex<ExecutorState>,
    state_tx: watch::Sender<ExecutorState>,
    broadcast: broadcast::Sender<OrchestratorEvent<D>>,
    /// Lifecycle feed. Run-loop and per-match tasks `.await` on it before
    /// taking the publish mutex.
    driver_tx: mpsc::Sender<DriverEvent<D>>,
    /// The composed sink (store / replay / etc.). All sink calls go
    /// through `Arc<dyn MatchSink>` so per-match tasks can share it.
    pub(crate) sink: Arc<dyn MatchSink<D>>,
}

impl<D: Descriptor> ExecutorInner<D> {
    /// Publish a lifecycle event (queued/started/finished/failed). Awaits
    /// the driver mpsc first; on `SendError` returns
    /// `OrchestratorInternalError::DriverDropped` so the run-loop can
    /// trigger root cancellation.
    pub(crate) async fn publish_lifecycle(
        &self,
        event: OrchestratorEvent<D>,
    ) -> Result<(), OrchestratorInternalError> {
        let driver = event
            .driver_event()
            .expect("publish_lifecycle called with per-turn MatchEvent");

        // 1. Driver mpsc: bounded, awaited, may backpressure.
        self.driver_tx
            .send(driver)
            .await
            .map_err(|_| OrchestratorInternalError::DriverDropped)?;

        // 2. Sync state-update + broadcast under publish mutex.
        let mut state = self.state.lock();
        apply_state_transition(&mut state, &event);
        // `send_replace` swaps without await; clone is cheap relative to
        // the sink path that just ran.
        let _ = self.state_tx.send_replace(state.clone());
        // broadcast.send returns SendError when there are no receivers,
        // expected, lossy by design.
        let _ = self.broadcast.send(event);
        Ok(())
    }

    /// Publish a per-turn `MatchEvent` to live observers. No state mutation,
    /// no driver mpsc, just broadcast.
    pub(crate) fn publish_per_turn(&self, event: OrchestratorEvent<D>) {
        let _ = self.broadcast.send(event);
    }

    /// Take an atomic snapshot of state alongside a fresh broadcast
    /// receiver. The mutex is held only across `.lock()` + clone +
    /// `subscribe()`; no awaits, so events published between the snapshot
    /// and the new receiver creation are impossible.
    pub(crate) fn atomic_subscribe(
        &self,
    ) -> (ExecutorState, broadcast::Receiver<OrchestratorEvent<D>>) {
        let state = self.state.lock();
        let snapshot = state.clone();
        let rx = self.broadcast.subscribe();
        drop(state);
        (snapshot, rx)
    }
}

fn apply_state_transition<D: Descriptor>(state: &mut ExecutorState, event: &OrchestratorEvent<D>) {
    match event {
        OrchestratorEvent::MatchQueued { .. } => {
            state.queued = state.queued.saturating_add(1);
        },
        OrchestratorEvent::MatchStarted { id, .. } => {
            state.queued = state.queued.saturating_sub(1);
            state.running.insert(*id, SystemTime::now());
        },
        OrchestratorEvent::MatchFinished { outcome } => {
            state.running.remove(&outcome.descriptor.match_id());
            state.finished = state.finished.saturating_add(1);
        },
        OrchestratorEvent::MatchFailed { failure } => {
            let id = failure.descriptor.match_id();
            // The match might have been queued-only (failed before
            // MatchStarted) or actually running. The two cases differ only
            // in which counter to decrement.
            if state.running.remove(&id).is_none() {
                state.queued = state.queued.saturating_sub(1);
            }
            state.failed = state.failed.saturating_add(1);
        },
        OrchestratorEvent::MatchEvent { .. } => {
            unreachable!("apply_state_transition called for per-turn event")
        },
    }
}

/// Concurrent match executor. Owns the run-loop task; drops it on `Drop`.
pub struct Orchestrator<D: Descriptor> {
    inner: Arc<ExecutorInner<D>>,
    submit_tx: mpsc::Sender<Matchup<D>>,
    root_cancel: CancellationToken,
    /// Aborts the run-loop if the orchestrator is dropped without
    /// `shutdown` being called.
    _run_loop: AbortOnDropHandle<()>,
    semaphore: Arc<Semaphore>,
    allocator: crate::id::MatchIdAllocator,
}

impl<D: Descriptor> Orchestrator<D> {
    /// Construct an orchestrator. Returns `(self, driver_rx)`. The caller
    /// (the "driver", typically an `EvalSession` or a CLI loop) consumes
    /// the lifecycle mpsc exclusively. Dropping `driver_rx` is fatal: the
    /// run-loop sees `SendError` on the next lifecycle publish, cancels
    /// every match, drains, and exits.
    pub fn new(
        config: OrchestratorConfig,
        sink: Arc<dyn MatchSink<D>>,
    ) -> (Self, mpsc::Receiver<DriverEvent<D>>) {
        let (submit_tx, submit_rx) = mpsc::channel::<Matchup<D>>(config.submit_capacity);
        let (driver_tx, driver_rx) = mpsc::channel::<DriverEvent<D>>(config.driver_events_capacity);
        let (broadcast_tx, _broadcast_rx) =
            broadcast::channel::<OrchestratorEvent<D>>(config.broadcast_capacity);
        let initial_state = ExecutorState::default();
        let (state_tx, _state_rx) = watch::channel(initial_state.clone());

        let inner = Arc::new(ExecutorInner {
            state: Mutex::new(initial_state),
            state_tx,
            broadcast: broadcast_tx,
            driver_tx,
            sink,
        });

        let semaphore = Arc::new(Semaphore::new(config.max_parallel));
        let root_cancel = CancellationToken::new();
        let run_cfg = Arc::new(RunConfig {
            setup_timing: config.setup_timing,
            playing_config: config.playing_config,
            handshake_timeout: config.handshake_timeout,
        });

        let inner_for_loop = inner.clone();
        let sem_for_loop = semaphore.clone();
        let cancel_for_loop = root_cancel.clone();
        let run_loop_handle = tokio::spawn(run_loop(
            inner_for_loop,
            submit_rx,
            sem_for_loop,
            cancel_for_loop,
            run_cfg,
        ));
        let _run_loop = AbortOnDropHandle::new(run_loop_handle);

        (
            Self {
                inner,
                submit_tx,
                root_cancel,
                _run_loop,
                semaphore,
                allocator: crate::id::MatchIdAllocator::new(),
            },
            driver_rx,
        )
    }

    /// Allocate a fresh `MatchId` from this orchestrator's monotonic space.
    /// Use it to populate the descriptor before [`Self::submit`].
    pub fn allocate_id(&self) -> MatchId {
        self.allocator.allocate()
    }

    /// Submit a matchup. Backpressure: if the submit channel is full,
    /// awaits until a slot frees. Returns `ShutDown` if the run-loop has
    /// already exited.
    pub async fn submit(&self, matchup: Matchup<D>) -> Result<(), OrchestratorError> {
        self.submit_tx
            .send(matchup)
            .await
            .map_err(|_| OrchestratorError::ShutDown)
    }

    /// Atomic `(state_snapshot, broadcast_receiver)`. Lifecycle effects
    /// reflected in the snapshot precede any subsequent broadcast item the
    /// returned receiver observes.
    pub fn subscribe(&self) -> (ExecutorState, broadcast::Receiver<OrchestratorEvent<D>>) {
        self.inner.atomic_subscribe()
    }

    /// Late lossy subscription to the broadcast stream. Same as
    /// `subscribe().1` but without the snapshot.
    pub fn events(&self) -> broadcast::Receiver<OrchestratorEvent<D>> {
        self.inner.broadcast.subscribe()
    }

    /// Watch over the executor state. Useful for one-shot snapshots and
    /// idle polling.
    pub fn state(&self) -> watch::Receiver<ExecutorState> {
        self.inner.state_tx.subscribe()
    }

    /// Available semaphore permits (`max_parallel - in_flight`).
    pub fn available_capacity(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// True when nothing is queued or running. Snapshot-based, may race
    /// with submit; use `state().changed()` for edge detection.
    pub fn idle(&self) -> bool {
        let s = self.inner.state.lock();
        s.queued == 0 && s.running.is_empty()
    }

    /// Trigger graceful shutdown: cancel the root token, close the submit
    /// channel, and wait for the run-loop to drain. Returns once the
    /// run-loop has exited.
    pub async fn shutdown(self) {
        self.root_cancel.cancel();
        // Drop submit_tx so any blocked sender returns; the run-loop's
        // `recv` returns None and breaks the loop. AbortOnDropHandle
        // aborts the run-loop task on drop.
        drop(self.submit_tx);
        drop(self._run_loop);
    }

    /// Synchronous abort: cancel the root token immediately. The run-loop
    /// is aborted at orchestrator drop time via `AbortOnDropHandle`. Use
    /// when graceful shutdown isn't needed.
    pub fn abort(&self) {
        self.root_cancel.cancel();
    }
}

async fn run_loop<D: Descriptor>(
    inner: Arc<ExecutorInner<D>>,
    mut submit_rx: mpsc::Receiver<Matchup<D>>,
    semaphore: Arc<Semaphore>,
    root_cancel: CancellationToken,
    run_cfg: Arc<RunConfig>,
) {
    let mut tasks: JoinSet<()> = JoinSet::new();

    loop {
        tokio::select! {
            biased;
            _ = root_cancel.cancelled() => break,
            joined = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Err(e)) = joined {
                    if e.is_panic() {
                        // run_match catches Match::run errors and emits
                        // MatchFailed { Panic }. If run_match itself
                        // panicked, no terminal was published.
                        error!(error = %e, "run_match task panicked unexpectedly");
                    } else if e.is_cancelled() {
                        // Force-aborted task. This shouldn't happen
                        // because we drop futures cooperatively, but it's
                        // legitimate during shutdown.
                        warn!("run_match task was force-aborted");
                    }
                }
            }
            maybe_item = submit_rx.recv() => {
                let Some(matchup) = maybe_item else { break; };
                let inner_c = inner.clone();
                let sem_c = semaphore.clone();
                let cfg_c = run_cfg.clone();
                let cancel = root_cancel.child_token();
                tasks.spawn(async move {
                    run_match::run_match(inner_c, matchup, cancel, sem_c, cfg_c).await;
                });
            }
        }
    }

    // Drain in-flight tasks. Each child sees root_cancel.cancelled() in
    // its biased select and bails through Drop on the run future. Wait
    // bounded so a misbehaving sink doesn't strand shutdown forever.
    let drain = async { while tasks.join_next().await.is_some() {} };
    if tokio::time::timeout(Duration::from_secs(10), drain)
        .await
        .is_err()
    {
        warn!("orchestrator shutdown drain timed out, aborting stragglers");
        tasks.shutdown().await;
    }
}
