//! Replayable tournament timing evidence and nested ETA candidates.
//!
//! This module is intentionally independent of the GUI and of wall clock.
//! A live session records monotonic observations; tests and later M1P/M2/M3
//! candidates replay the same `TournamentTimingTrace` walk-forward.

use std::collections::{HashMap, HashSet};

use parking_lot::Mutex;
use pyrat_orchestrator::{
    AttemptTerminal, ExecutionPhase, ExecutionTimingEvent, MatchId, MonotonicTime, TimingObserver,
};
use serde::{Deserialize, Serialize};

use crate::{DispatchPolicy, EvalMatchDescriptor, PlannedSlot};

pub const M0_MODEL_ID: &str = "terminal_slot_throughput_v0";
pub const M1_MODEL_ID: &str = "global_attempt_scheduler_v1";
pub const M1P_MODEL_ID: &str = "phase_turn_progress_probe_v1";
pub const TIMING_TRACE_SCHEMA_VERSION: u32 = 2;

/// ETA candidate evaluated against a timing trace.  Keeping candidate choice
/// at this seam makes every model consume the same walk-forward evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EtaModel {
    M0,
    M1,
    #[serde(alias = "m2")]
    M1P,
}

impl EtaModel {
    pub const fn id(self) -> &'static str {
        match self {
            Self::M0 => M0_MODEL_ID,
            Self::M1 => M1_MODEL_ID,
            Self::M1P => M1P_MODEL_ID,
        }
    }
}

/// Static schedule plus the monotonic execution facts observed so far.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TournamentTimingTrace {
    pub schema_version: u32,
    /// Methodology that produced the realized queue order. Schema-v1 traces
    /// predate the field and decode to the historical pair-major policy.
    #[serde(default)]
    pub dispatch_policy: DispatchPolicy,
    pub max_parallel: usize,
    pub max_failures_per_slot: u32,
    pub slots: Vec<PlannedSlot>,
    pub events: Vec<ExecutionTimingEvent<EvalMatchDescriptor>>,
}

impl TournamentTimingTrace {
    /// Stable replay order.  The source monotonic timestamp wins; sequence is
    /// only a tie-breaker for boundaries observed at the same instant.
    pub fn normalized(mut self) -> Self {
        self.events
            .sort_by_key(|event| (event.observed_at, event.sequence));
        self
    }
}

/// In-memory production recorder installed on `EvalSession`'s orchestrator.
pub(crate) struct TournamentTimingRecorder {
    dispatch_policy: DispatchPolicy,
    max_parallel: usize,
    max_failures_per_slot: u32,
    slots: Vec<PlannedSlot>,
    events: Mutex<Vec<ExecutionTimingEvent<EvalMatchDescriptor>>>,
}

/// Cloneable read-only handle that outlives `EvalSession::join`, allowing a
/// completed production trace to be captured without keeping the session
/// owner alive.
#[derive(Clone)]
pub struct TournamentTimingReader {
    recorder: std::sync::Arc<TournamentTimingRecorder>,
}

impl TournamentTimingReader {
    pub(crate) fn new(recorder: std::sync::Arc<TournamentTimingRecorder>) -> Self {
        Self { recorder }
    }

    pub fn snapshot(&self) -> TournamentTimingTrace {
        self.recorder.snapshot()
    }
}

impl TournamentTimingRecorder {
    pub(crate) fn new(
        dispatch_policy: DispatchPolicy,
        max_parallel: usize,
        max_failures_per_slot: u32,
        slots: Vec<PlannedSlot>,
    ) -> Self {
        Self {
            dispatch_policy,
            max_parallel,
            max_failures_per_slot,
            slots,
            events: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn snapshot(&self) -> TournamentTimingTrace {
        TournamentTimingTrace {
            schema_version: TIMING_TRACE_SCHEMA_VERSION,
            dispatch_policy: self.dispatch_policy,
            max_parallel: self.max_parallel,
            max_failures_per_slot: self.max_failures_per_slot,
            slots: self.slots.clone(),
            events: self.events.lock().clone(),
        }
        .normalized()
    }
}

impl TimingObserver<EvalMatchDescriptor> for TournamentTimingRecorder {
    fn observe(&self, event: ExecutionTimingEvent<EvalMatchDescriptor>) {
        self.events.lock().push(event);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EtaState {
    Learning,
    Ready,
    Waiting,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtaEstimate {
    pub model: String,
    pub state: EtaState,
    pub observed_terminal_slots: u32,
    pub remaining_slots: u32,
    pub seconds: Option<u64>,
}

/// One estimate emitted after each replayed observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtaReplayPoint {
    pub at: MonotonicTime,
    pub estimate: EtaEstimate,
}

/// One model's walk-forward prediction scored against the eventual finish.
/// The score is absent while the model is still learning or waiting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtaScoredEstimate {
    pub estimate: EtaEstimate,
    pub signed_error_seconds: Option<i64>,
    pub absolute_error_seconds: Option<u64>,
}

/// Every current candidate evaluated at the same observation boundary.
/// Ground truth is attached only after all models have replayed each prefix;
/// it is never fed back into an estimator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtaModelComparisonPoint {
    pub at: MonotonicTime,
    pub actual_remaining_seconds: u64,
    pub m0: EtaScoredEstimate,
    pub m1: EtaScoredEstimate,
    #[serde(alias = "m2")]
    pub m1p: EtaScoredEstimate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveScheduleAttempt {
    pub descriptor: EvalMatchDescriptor,
    pub phase: ExecutionPhase,
    pub active_concurrency: usize,
}

/// Read-only schedule inventory reconstructed from the same trace candidates
/// consume.  `remaining` excludes slots with a currently active attempt;
/// failed non-durable attempts return to `remaining` once their terminal is
/// produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TournamentScheduleSnapshot {
    pub at: Option<MonotonicTime>,
    pub active: Vec<ActiveScheduleAttempt>,
    pub remaining: Vec<PlannedSlot>,
    pub terminal: Vec<PlannedSlot>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EtaTraceError {
    #[error("unsupported timing trace schema {got}; expected {expected}")]
    UnsupportedSchema { expected: u32, got: u32 },
    #[error("timing trace requires at least one parallel worker")]
    ZeroParallelism,
    #[error("timing trace requires a positive failure budget")]
    ZeroFailureBudget,
    #[error("event references slot absent from schedule: {0:?}")]
    UnknownSlot(PlannedSlot),
    #[error("schedule contains duplicate slot: {0:?}")]
    DuplicateScheduleSlot(PlannedSlot),
    #[error("terminal observation for match {0} arrived before its queue observation")]
    TerminalBeforeQueue(MatchId),
    #[error("timing event reports concurrency {observed} above configured maximum {max_parallel}")]
    ConcurrencyExceedsLimit {
        observed: usize,
        max_parallel: usize,
    },
    #[error(
        "walk-forward scoring requires a complete trace; observed {observed_terminal_slots} of {total_slots} terminal slots"
    )]
    IncompleteTrace {
        observed_terminal_slots: u32,
        total_slots: u32,
    },
}

/// Replay one ETA candidate strictly walk-forward.  The seed is part of the
/// stable seam for later stochastic candidates; M0, M1, and M1P are
/// deterministic.
pub fn replay_eta(
    trace: &TournamentTimingTrace,
    model: EtaModel,
    _simulation_seed: u64,
) -> Result<Vec<EtaReplayPoint>, EtaTraceError> {
    let normalized = trace.clone().normalized();
    let schedule = validate_trace(&normalized)?;
    let mut replay = ReplayState::new(
        normalized.slots.len(),
        normalized.max_parallel,
        normalized.max_failures_per_slot,
    );
    let mut points = Vec::with_capacity(normalized.events.len());

    for event in &normalized.events {
        let slot = event.descriptor.slot_key();
        if !schedule.contains(&slot) {
            return Err(EtaTraceError::UnknownSlot(slot));
        }
        replay.apply(event)?;
        points.push(EtaReplayPoint {
            at: event.observed_at,
            estimate: replay.estimate(model, event.observed_at, &normalized.slots),
        });
    }
    Ok(points)
}

/// Replay the current terminal-slot throughput baseline.
pub fn replay_m0(
    trace: &TournamentTimingTrace,
    simulation_seed: u64,
) -> Result<Vec<EtaReplayPoint>, EtaTraceError> {
    replay_eta(trace, EtaModel::M0, simulation_seed)
}

/// Replay M1: global completed-attempt duration residuals folded through the
/// exact currently occupied workers and final partial batch.
pub fn replay_m1(
    trace: &TournamentTimingTrace,
    simulation_seed: u64,
) -> Result<Vec<EtaReplayPoint>, EtaTraceError> {
    replay_eta(trace, EtaModel::M1, simulation_seed)
}

/// Replay M1P: M1's scheduler fold with active attempt service conditioned
/// on the latest observed execution phase and turn. Pending work remains
/// global; the partially pooled semi-mechanistic candidate retains the M2
/// name.
pub fn replay_m1p(
    trace: &TournamentTimingTrace,
    simulation_seed: u64,
) -> Result<Vec<EtaReplayPoint>, EtaTraceError> {
    replay_eta(trace, EtaModel::M1P, simulation_seed)
}

/// Score every current candidate at each observation boundary against
/// eventual remaining wall time.  This is the data seam for the HTML decision
/// surface.
pub fn compare_eta_models_walk_forward(
    trace: &TournamentTimingTrace,
    simulation_seed: u64,
) -> Result<Vec<EtaModelComparisonPoint>, EtaTraceError> {
    let m0 = replay_m0(trace, simulation_seed)?;
    let m1 = replay_m1(trace, simulation_seed)?;
    let m1p = replay_m1p(trace, simulation_seed)?;
    let total_slots = u32::try_from(trace.slots.len()).unwrap_or(u32::MAX);
    let completion_at = m0
        .iter()
        .find(|point| point.estimate.state == EtaState::Complete)
        .map(|point| point.at)
        .ok_or_else(|| EtaTraceError::IncompleteTrace {
            observed_terminal_slots: m0
                .last()
                .map_or(0, |point| point.estimate.observed_terminal_slots),
            total_slots,
        })?;

    Ok(m0
        .into_iter()
        .zip(m1)
        .zip(m1p)
        .map(|((m0, m1), m1p)| {
            debug_assert_eq!(m0.at, m1.at);
            debug_assert_eq!(m0.at, m1p.at);
            let actual_remaining_seconds =
                micros_to_seconds_ceil(completion_at.micros.saturating_sub(m0.at.micros));
            EtaModelComparisonPoint {
                at: m0.at,
                actual_remaining_seconds,
                m0: score_estimate(m0.estimate, actual_remaining_seconds),
                m1: score_estimate(m1.estimate, actual_remaining_seconds),
                m1p: score_estimate(m1p.estimate, actual_remaining_seconds),
            }
        })
        .collect())
}

/// Fold the current trace into active, remaining, and durable-terminal slot
/// inventories without mutating the planner.
pub fn schedule_snapshot(
    trace: &TournamentTimingTrace,
) -> Result<TournamentScheduleSnapshot, EtaTraceError> {
    let normalized = trace.clone().normalized();
    let schedule = validate_trace(&normalized)?;
    let mut replay = ReplayState::new(
        normalized.slots.len(),
        normalized.max_parallel,
        normalized.max_failures_per_slot,
    );
    for event in &normalized.events {
        let slot = event.descriptor.slot_key();
        if !schedule.contains(&slot) {
            return Err(EtaTraceError::UnknownSlot(slot));
        }
        replay.apply(event)?;
    }
    Ok(replay.schedule_snapshot(
        &normalized.slots,
        normalized.events.last().map(|event| event.observed_at),
    ))
}

fn validate_trace(trace: &TournamentTimingTrace) -> Result<HashSet<PlannedSlot>, EtaTraceError> {
    if trace.schema_version == 0 || trace.schema_version > TIMING_TRACE_SCHEMA_VERSION {
        return Err(EtaTraceError::UnsupportedSchema {
            expected: TIMING_TRACE_SCHEMA_VERSION,
            got: trace.schema_version,
        });
    }
    if trace.max_parallel == 0 {
        return Err(EtaTraceError::ZeroParallelism);
    }
    if trace.max_failures_per_slot == 0 {
        return Err(EtaTraceError::ZeroFailureBudget);
    }
    let mut schedule = HashSet::with_capacity(trace.slots.len());
    for slot in &trace.slots {
        if !schedule.insert(slot.clone()) {
            return Err(EtaTraceError::DuplicateScheduleSlot(slot.clone()));
        }
    }
    for event in &trace.events {
        if event.active_concurrency > trace.max_parallel {
            return Err(EtaTraceError::ConcurrencyExceedsLimit {
                observed: event.active_concurrency,
                max_parallel: trace.max_parallel,
            });
        }
    }
    Ok(schedule)
}

struct ReplayState {
    total_slots: usize,
    max_parallel: usize,
    max_failures_per_slot: u32,
    first_queued_at: Option<MonotonicTime>,
    last_progress_at: Option<MonotonicTime>,
    attempt_slots: HashMap<MatchId, PlannedSlot>,
    failures: HashMap<PlannedSlot, u32>,
    terminal_slots: HashSet<PlannedSlot>,
    active_matches: HashSet<MatchId>,
    open_attempts: HashMap<MatchId, ActiveScheduleAttempt>,
    permit_acquired_at: HashMap<MatchId, MonotonicTime>,
    terminal_attempts: HashSet<MatchId>,
    completed_attempt_micros: Vec<u64>,
    attempt_checkpoints: HashMap<MatchId, Vec<TimedServiceCheckpoint>>,
    completed_attempt_profiles: Vec<CompletedAttemptProfile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceCheckpoint {
    PermitAcquired,
    LaunchStarted,
    HandshakeCompleted,
    ConfigureStarted,
    PreprocessingStarted,
    SetupCompleted,
    PlayingStarted,
    ActionCollectionStarted(u16),
    ActionsCollected(u16),
    TurnCompleted(u16),
    TerminalProduced,
    TerminalPersisted,
    TerminalPersistenceFailed,
}

impl ServiceCheckpoint {
    fn from_phase(phase: &ExecutionPhase) -> Option<Self> {
        match phase {
            ExecutionPhase::Queued { .. } | ExecutionPhase::PermitReleased => None,
            ExecutionPhase::PermitAcquired => Some(Self::PermitAcquired),
            ExecutionPhase::LaunchStarted => Some(Self::LaunchStarted),
            ExecutionPhase::HandshakeCompleted { .. } => Some(Self::HandshakeCompleted),
            ExecutionPhase::ConfigureStarted => Some(Self::ConfigureStarted),
            ExecutionPhase::PreprocessingStarted => Some(Self::PreprocessingStarted),
            ExecutionPhase::SetupCompleted => Some(Self::SetupCompleted),
            ExecutionPhase::PlayingStarted => Some(Self::PlayingStarted),
            ExecutionPhase::ActionCollectionStarted { turn } => {
                Some(Self::ActionCollectionStarted(*turn))
            },
            ExecutionPhase::ActionsCollected { turn } => Some(Self::ActionsCollected(*turn)),
            ExecutionPhase::TurnCompleted { turn } => Some(Self::TurnCompleted(*turn)),
            ExecutionPhase::TerminalProduced { .. } => Some(Self::TerminalProduced),
            ExecutionPhase::TerminalPersisted { .. } => Some(Self::TerminalPersisted),
            ExecutionPhase::TerminalPersistenceFailed => Some(Self::TerminalPersistenceFailed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct TimedServiceCheckpoint {
    checkpoint: ServiceCheckpoint,
    observed_at: MonotonicTime,
}

struct CompletedAttemptProfile {
    total_micros: u64,
    checkpoints: Vec<(ServiceCheckpoint, u64)>,
}

#[derive(Debug, Clone, Copy)]
enum ActiveResidualModel {
    Age,
    PhaseTurn,
}

impl ReplayState {
    fn new(total_slots: usize, max_parallel: usize, max_failures_per_slot: u32) -> Self {
        Self {
            total_slots,
            max_parallel,
            max_failures_per_slot,
            first_queued_at: None,
            last_progress_at: None,
            attempt_slots: HashMap::new(),
            failures: HashMap::new(),
            terminal_slots: HashSet::new(),
            active_matches: HashSet::new(),
            open_attempts: HashMap::new(),
            permit_acquired_at: HashMap::new(),
            terminal_attempts: HashSet::new(),
            completed_attempt_micros: Vec::new(),
            attempt_checkpoints: HashMap::new(),
            completed_attempt_profiles: Vec::new(),
        }
    }

    fn apply(
        &mut self,
        event: &ExecutionTimingEvent<EvalMatchDescriptor>,
    ) -> Result<(), EtaTraceError> {
        let match_id = event.descriptor.match_id;
        if let Some(active) = self.open_attempts.get_mut(&match_id) {
            active.phase = event.phase.clone();
            active.active_concurrency = event.active_concurrency;
        }
        if self.permit_acquired_at.contains_key(&match_id) {
            if let Some(checkpoint) = ServiceCheckpoint::from_phase(&event.phase) {
                self.attempt_checkpoints.entry(match_id).or_default().push(
                    TimedServiceCheckpoint {
                        checkpoint,
                        observed_at: event.observed_at,
                    },
                );
            }
        }
        match event.phase {
            ExecutionPhase::Queued { .. } => {
                self.first_queued_at.get_or_insert(event.observed_at);
                self.attempt_slots
                    .insert(match_id, event.descriptor.slot_key());
                self.open_attempts.insert(
                    match_id,
                    ActiveScheduleAttempt {
                        descriptor: event.descriptor.clone(),
                        phase: event.phase.clone(),
                        active_concurrency: event.active_concurrency,
                    },
                );
            },
            ExecutionPhase::PermitAcquired => {
                self.active_matches.insert(match_id);
                if self.active_matches.len() > self.max_parallel {
                    return Err(EtaTraceError::ConcurrencyExceedsLimit {
                        observed: self.active_matches.len(),
                        max_parallel: self.max_parallel,
                    });
                }
                self.permit_acquired_at.insert(match_id, event.observed_at);
                self.attempt_checkpoints.insert(
                    match_id,
                    vec![TimedServiceCheckpoint {
                        checkpoint: ServiceCheckpoint::PermitAcquired,
                        observed_at: event.observed_at,
                    }],
                );
            },
            ExecutionPhase::TerminalPersisted { outcome } => {
                self.apply_terminal(match_id, outcome, event.observed_at)?;
                self.terminal_attempts.insert(match_id);
                self.open_attempts.remove(&match_id);
            },
            ExecutionPhase::TerminalProduced {
                outcome:
                    AttemptTerminal::Failed {
                        durable_record: false,
                    },
            }
            | ExecutionPhase::TerminalPersistenceFailed => {
                self.terminal_attempts.insert(match_id);
                self.open_attempts.remove(&match_id);
            },
            // A produced durable terminal is still occupying its schedule
            // slot until the required sink confirms persistence.  Keeping it
            // active prevents a live snapshot from briefly offering the slot
            // as remaining work during store latency.
            ExecutionPhase::TerminalProduced { .. } => {},
            ExecutionPhase::PermitReleased => {
                self.active_matches.remove(&match_id);
                self.open_attempts.remove(&match_id);
                self.terminal_attempts.remove(&match_id);
                if let Some(acquired_at) = self.permit_acquired_at.remove(&match_id) {
                    let total_micros = event
                        .observed_at
                        .micros
                        .saturating_sub(acquired_at.micros)
                        .max(1);
                    self.completed_attempt_micros.push(total_micros);
                    let checkpoints = self
                        .attempt_checkpoints
                        .remove(&match_id)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|timed| {
                            (
                                timed.checkpoint,
                                timed
                                    .observed_at
                                    .micros
                                    .saturating_sub(acquired_at.micros)
                                    .min(total_micros),
                            )
                        })
                        .collect();
                    self.completed_attempt_profiles
                        .push(CompletedAttemptProfile {
                            total_micros,
                            checkpoints,
                        });
                }
            },
            _ => {},
        }
        Ok(())
    }

    fn schedule_snapshot(
        &self,
        slots: &[PlannedSlot],
        at: Option<MonotonicTime>,
    ) -> TournamentScheduleSnapshot {
        let active_slots: HashSet<_> = self
            .open_attempts
            .values()
            .map(|attempt| attempt.descriptor.slot_key())
            .collect();
        let mut active: Vec<_> = self.open_attempts.values().cloned().collect();
        active.sort_by_key(|attempt| attempt.descriptor.match_id);
        let remaining = slots
            .iter()
            .filter(|slot| !self.terminal_slots.contains(*slot) && !active_slots.contains(*slot))
            .cloned()
            .collect();
        let terminal = slots
            .iter()
            .filter(|slot| self.terminal_slots.contains(*slot))
            .cloned()
            .collect();
        TournamentScheduleSnapshot {
            at,
            active,
            remaining,
            terminal,
        }
    }

    fn apply_terminal(
        &mut self,
        match_id: MatchId,
        outcome: AttemptTerminal,
        observed_at: MonotonicTime,
    ) -> Result<(), EtaTraceError> {
        let slot = self
            .attempt_slots
            .get(&match_id)
            .cloned()
            .ok_or(EtaTraceError::TerminalBeforeQueue(match_id))?;
        let became_terminal = match outcome {
            AttemptTerminal::Finished => self.terminal_slots.insert(slot),
            AttemptTerminal::Failed {
                durable_record: true,
            } => {
                let count = self.failures.entry(slot.clone()).or_default();
                *count = count.saturating_add(1);
                *count >= self.max_failures_per_slot && self.terminal_slots.insert(slot)
            },
            AttemptTerminal::Failed {
                durable_record: false,
            } => false,
        };
        if became_terminal {
            self.last_progress_at = Some(observed_at);
        }
        Ok(())
    }

    fn estimate(&self, model: EtaModel, now: MonotonicTime, slots: &[PlannedSlot]) -> EtaEstimate {
        match model {
            EtaModel::M0 => self.estimate_m0(now),
            EtaModel::M1 => self.estimate_m1(now, slots),
            EtaModel::M1P => self.estimate_m1p(now, slots),
        }
    }

    fn estimate_m0(&self, now: MonotonicTime) -> EtaEstimate {
        let observed_terminal_slots = u32::try_from(self.terminal_slots.len()).unwrap_or(u32::MAX);
        let remaining_slots =
            u32::try_from(self.total_slots.saturating_sub(self.terminal_slots.len()))
                .unwrap_or(u32::MAX);
        if remaining_slots == 0 {
            return self.complete(M0_MODEL_ID, observed_terminal_slots);
        }

        let minimum_samples = self.total_slots.min(self.max_parallel.clamp(2, 6));
        let Some(started_at) = self.first_queued_at else {
            return self.learning(M0_MODEL_ID, observed_terminal_slots, remaining_slots);
        };
        let elapsed_micros = now.micros.saturating_sub(started_at.micros);
        if self.terminal_slots.len() < minimum_samples || elapsed_micros < 2_000_000 {
            return self.learning_or_waiting(
                M0_MODEL_ID,
                now,
                observed_terminal_slots,
                remaining_slots,
                20_000_000,
            );
        }

        let observation_end = self.last_progress_at.unwrap_or(now);
        let observed_micros = observation_end
            .micros
            .saturating_sub(started_at.micros)
            .max(1_000_000);
        let seconds_per_terminal =
            observed_micros as f64 / 1_000_000.0 / f64::from(observed_terminal_slots);
        let stall_micros = ((seconds_per_terminal * 2.5).max(20.0) * 1_000_000.0) as u64;
        if self.active_matches.is_empty()
            && now.micros.saturating_sub(observation_end.micros) >= stall_micros
        {
            return EtaEstimate {
                model: M0_MODEL_ID.into(),
                state: EtaState::Waiting,
                observed_terminal_slots,
                remaining_slots,
                seconds: None,
            };
        }

        EtaEstimate {
            model: M0_MODEL_ID.into(),
            state: EtaState::Ready,
            observed_terminal_slots,
            remaining_slots,
            seconds: Some(
                (f64::from(remaining_slots) * seconds_per_terminal)
                    .ceil()
                    .max(1.0) as u64,
            ),
        }
    }

    fn estimate_m1(&self, now: MonotonicTime, slots: &[PlannedSlot]) -> EtaEstimate {
        self.estimate_scheduler(now, slots, M1_MODEL_ID, ActiveResidualModel::Age)
    }

    fn estimate_m1p(&self, now: MonotonicTime, slots: &[PlannedSlot]) -> EtaEstimate {
        self.estimate_scheduler(now, slots, M1P_MODEL_ID, ActiveResidualModel::PhaseTurn)
    }

    fn estimate_scheduler(
        &self,
        now: MonotonicTime,
        slots: &[PlannedSlot],
        model_id: &str,
        active_residual_model: ActiveResidualModel,
    ) -> EtaEstimate {
        let observed_terminal_slots = u32::try_from(self.terminal_slots.len()).unwrap_or(u32::MAX);
        let remaining_slots =
            u32::try_from(self.total_slots.saturating_sub(self.terminal_slots.len()))
                .unwrap_or(u32::MAX);
        if remaining_slots == 0 {
            return self.complete(model_id, observed_terminal_slots);
        }

        let Some(mean_attempt_micros) = self.mean_attempt_micros() else {
            return self.learning_or_waiting(
                model_id,
                now,
                observed_terminal_slots,
                remaining_slots,
                20_000_000,
            );
        };
        let observation_end = self
            .last_progress_at
            .or(self.first_queued_at)
            .unwrap_or(now);
        let stall_micros = mean_attempt_micros
            .saturating_mul(5)
            .checked_div(2)
            .unwrap_or(u64::MAX)
            .max(20_000_000);
        if self.active_matches.is_empty()
            && now.micros.saturating_sub(observation_end.micros) >= stall_micros
        {
            return EtaEstimate {
                model: model_id.into(),
                state: EtaState::Waiting,
                observed_terminal_slots,
                remaining_slots,
                seconds: None,
            };
        }

        let makespan_micros =
            self.scheduler_makespan_micros(now, slots, mean_attempt_micros, active_residual_model);
        EtaEstimate {
            model: model_id.into(),
            state: EtaState::Ready,
            observed_terminal_slots,
            remaining_slots,
            seconds: Some(micros_to_seconds_ceil(makespan_micros).max(1)),
        }
    }

    fn mean_attempt_micros(&self) -> Option<u64> {
        if self.completed_attempt_micros.is_empty() {
            return None;
        }
        let total: u128 = self
            .completed_attempt_micros
            .iter()
            .map(|&duration| u128::from(duration))
            .sum();
        let mean = total / self.completed_attempt_micros.len() as u128;
        Some(u64::try_from(mean).unwrap_or(u64::MAX).max(1))
    }

    /// List-schedule the outstanding work on the workers as they are occupied
    /// now. Every not-yet-started attempt uses the same global completed-
    /// attempt mean; only the active residual differs between M1 and M1P.
    fn scheduler_makespan_micros(
        &self,
        now: MonotonicTime,
        slots: &[PlannedSlot],
        mean_attempt_micros: u64,
        active_residual_model: ActiveResidualModel,
    ) -> u64 {
        let mut workers = vec![0; self.max_parallel];
        let mut covered_slots = HashSet::with_capacity(self.active_matches.len());
        let mut active_matches: Vec<_> = self.active_matches.iter().copied().collect();
        active_matches.sort_unstable();

        for (worker, match_id) in workers.iter_mut().zip(active_matches) {
            if self.terminal_attempts.contains(&match_id) {
                // The terminal boundary has already happened. The release
                // event may still be in flight, but no attempt service remains
                // to model at this granularity.
                *worker = 0;
            } else {
                let age = self
                    .permit_acquired_at
                    .get(&match_id)
                    .map_or(0, |started_at| now.micros.saturating_sub(started_at.micros));
                *worker = match active_residual_model {
                    ActiveResidualModel::Age => mean_attempt_micros.saturating_sub(age),
                    ActiveResidualModel::PhaseTurn => self
                        .phase_turn_remaining_micros(match_id)
                        .unwrap_or_else(|| mean_attempt_micros.saturating_sub(age)),
                };
                if let Some(slot) = self.attempt_slots.get(&match_id) {
                    if !self.terminal_slots.contains(slot) {
                        covered_slots.insert(slot.clone());
                    }
                }
            }
        }

        let mut queued_matches: Vec<_> = self
            .open_attempts
            .keys()
            .filter(|match_id| !self.active_matches.contains(match_id))
            .copied()
            .collect();
        queued_matches.sort_unstable();
        for match_id in queued_matches {
            let Some(slot) = self.attempt_slots.get(&match_id) else {
                continue;
            };
            if !self.terminal_slots.contains(slot) && covered_slots.insert(slot.clone()) {
                schedule_job(&mut workers, mean_attempt_micros);
            }
        }

        for slot in slots {
            if !self.terminal_slots.contains(slot) && covered_slots.insert(slot.clone()) {
                schedule_job(&mut workers, mean_attempt_micros);
            }
        }
        workers.into_iter().max().unwrap_or(0)
    }

    /// Estimate an active attempt from the last phase boundary it crossed.
    /// Exact turn-bearing checkpoints remain distinct.  The residual is held
    /// at that boundary until new progress arrives: conditioning on time spent
    /// inside a still-open phase would be the censored-survival extension
    /// reserved for M3, not this smallest M1P delta.
    fn phase_turn_remaining_micros(&self, match_id: MatchId) -> Option<u64> {
        let checkpoint = self
            .open_attempts
            .get(&match_id)
            .and_then(|attempt| ServiceCheckpoint::from_phase(&attempt.phase))?;
        mean_u64(
            self.completed_attempt_profiles
                .iter()
                .filter_map(|profile| {
                    profile
                        .checkpoints
                        .iter()
                        .rev()
                        .find(|(candidate, _)| *candidate == checkpoint)
                        .map(|(_, elapsed_micros)| {
                            profile.total_micros.saturating_sub(*elapsed_micros)
                        })
                }),
        )
    }

    fn complete(&self, model: &str, observed_terminal_slots: u32) -> EtaEstimate {
        EtaEstimate {
            model: model.into(),
            state: EtaState::Complete,
            observed_terminal_slots,
            remaining_slots: 0,
            seconds: None,
        }
    }

    fn learning(
        &self,
        model: &str,
        observed_terminal_slots: u32,
        remaining_slots: u32,
    ) -> EtaEstimate {
        EtaEstimate {
            model: model.into(),
            state: EtaState::Learning,
            observed_terminal_slots,
            remaining_slots,
            seconds: None,
        }
    }

    fn learning_or_waiting(
        &self,
        model: &str,
        now: MonotonicTime,
        observed_terminal_slots: u32,
        remaining_slots: u32,
        threshold_micros: u64,
    ) -> EtaEstimate {
        let last_progress = self
            .last_progress_at
            .or(self.first_queued_at)
            .unwrap_or(now);
        if self.active_matches.is_empty()
            && now.micros.saturating_sub(last_progress.micros) >= threshold_micros
        {
            EtaEstimate {
                model: model.into(),
                state: EtaState::Waiting,
                observed_terminal_slots,
                remaining_slots,
                seconds: None,
            }
        } else {
            self.learning(model, observed_terminal_slots, remaining_slots)
        }
    }
}

fn schedule_job(workers: &mut [u64], duration_micros: u64) {
    if let Some(worker) = workers.iter_mut().min() {
        *worker = worker.saturating_add(duration_micros);
    }
}

fn mean_u64(values: impl Iterator<Item = u64>) -> Option<u64> {
    let (total, count) = values.fold((0_u128, 0_u128), |(total, count), value| {
        (total + u128::from(value), count + 1)
    });
    (count > 0).then(|| u64::try_from(total / count).unwrap_or(u64::MAX))
}

fn micros_to_seconds_ceil(micros: u64) -> u64 {
    micros / 1_000_000 + u64::from(!micros.is_multiple_of(1_000_000))
}

fn score_estimate(estimate: EtaEstimate, actual_remaining_seconds: u64) -> EtaScoredEstimate {
    let predicted_seconds = match estimate.state {
        EtaState::Complete => Some(0),
        EtaState::Ready => estimate.seconds,
        EtaState::Learning | EtaState::Waiting => None,
    };
    let (signed_error_seconds, absolute_error_seconds) =
        predicted_seconds.map_or((None, None), |predicted| {
            let signed = i128::from(predicted) - i128::from(actual_remaining_seconds);
            (
                Some(signed.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64),
                Some(predicted.abs_diff(actual_remaining_seconds)),
            )
        });
    EtaScoredEstimate {
        estimate,
        signed_error_seconds,
        absolute_error_seconds,
    }
}

#[cfg(test)]
mod tests {
    use pyrat_eval_store::{SeatOrientation, TournamentId};
    use pyrat_orchestrator::{ExecutionTimingEvent, MatchId};

    use super::*;

    fn slot(rep: u32) -> PlannedSlot {
        PlannedSlot {
            player1_id: "a".into(),
            player2_id: "b".into(),
            game_config_id: "gc".into(),
            repetition_index: rep,
            orientation: SeatOrientation::Canonical,
        }
    }

    fn descriptor(match_id: u64, rep: u32, attempt_index: u32) -> EvalMatchDescriptor {
        EvalMatchDescriptor {
            match_id: MatchId(match_id),
            tournament_id: TournamentId(1),
            game_config_id: "gc".into(),
            player1_id: "a".into(),
            player2_id: "b".into(),
            seed: rep as u64,
            repetition_index: rep,
            attempt_index,
            orientation: SeatOrientation::Canonical,
            planned_at: std::time::SystemTime::UNIX_EPOCH,
        }
    }

    fn event(
        sequence: u64,
        micros: u64,
        descriptor: EvalMatchDescriptor,
        active_concurrency: usize,
        phase: ExecutionPhase,
    ) -> ExecutionTimingEvent<EvalMatchDescriptor> {
        ExecutionTimingEvent {
            sequence,
            observed_at: MonotonicTime::from_micros(micros),
            active_concurrency,
            descriptor,
            phase,
        }
    }

    struct SyntheticTrace {
        max_parallel: usize,
        slot_count: u32,
        events: Vec<ExecutionTimingEvent<EvalMatchDescriptor>>,
    }

    impl SyntheticTrace {
        fn new(max_parallel: usize, slot_count: u32) -> Self {
            Self {
                max_parallel,
                slot_count,
                events: Vec::new(),
            }
        }

        fn push(
            &mut self,
            micros: u64,
            descriptor: &EvalMatchDescriptor,
            active_concurrency: usize,
            phase: ExecutionPhase,
        ) {
            let sequence = self.events.len() as u64;
            self.events.push(event(
                sequence,
                micros,
                descriptor.clone(),
                active_concurrency,
                phase,
            ));
        }

        fn finish(self) -> TournamentTimingTrace {
            TournamentTimingTrace {
                schema_version: TIMING_TRACE_SCHEMA_VERSION,
                dispatch_policy: DispatchPolicy::PairMajorV1,
                max_parallel: self.max_parallel,
                max_failures_per_slot: 1,
                slots: (0..self.slot_count).map(slot).collect(),
                events: self.events,
            }
        }
    }

    fn final_partial_batch_trace() -> TournamentTimingTrace {
        let d0 = descriptor(0, 0, 0);
        let d1 = descriptor(1, 1, 0);
        let d2 = descriptor(2, 2, 0);
        let mut trace = SyntheticTrace::new(2, 3);
        trace.push(
            0,
            &d0,
            0,
            ExecutionPhase::Queued {
                players: ["a".into(), "b".into()],
            },
        );
        trace.push(
            0,
            &d1,
            0,
            ExecutionPhase::Queued {
                players: ["a".into(), "b".into()],
            },
        );
        trace.push(0, &d0, 1, ExecutionPhase::PermitAcquired);
        trace.push(0, &d1, 2, ExecutionPhase::PermitAcquired);
        trace.push(
            10_000_000,
            &d0,
            2,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Finished,
            },
        );
        trace.push(
            10_000_000,
            &d1,
            2,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Finished,
            },
        );
        trace.push(10_000_000, &d0, 1, ExecutionPhase::PermitReleased);
        trace.push(10_000_000, &d1, 0, ExecutionPhase::PermitReleased);
        trace.push(
            10_000_000,
            &d2,
            0,
            ExecutionPhase::Queued {
                players: ["a".into(), "b".into()],
            },
        );
        trace.push(10_000_000, &d2, 1, ExecutionPhase::PermitAcquired);
        trace.push(
            20_000_000,
            &d2,
            1,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Finished,
            },
        );
        trace.push(20_000_000, &d2, 0, ExecutionPhase::PermitReleased);
        trace.finish()
    }

    fn active_straggler_trace() -> TournamentTimingTrace {
        let d0 = descriptor(0, 0, 0);
        let d1 = descriptor(1, 1, 0);
        let d2 = descriptor(2, 2, 0);
        let d3 = descriptor(3, 3, 0);
        let mut trace = SyntheticTrace::new(2, 4);
        trace.push(
            0,
            &d0,
            0,
            ExecutionPhase::Queued {
                players: ["a".into(), "b".into()],
            },
        );
        trace.push(
            0,
            &d1,
            0,
            ExecutionPhase::Queued {
                players: ["a".into(), "b".into()],
            },
        );
        trace.push(0, &d0, 1, ExecutionPhase::PermitAcquired);
        trace.push(0, &d1, 2, ExecutionPhase::PermitAcquired);
        trace.push(1_000_000, &d0, 2, ExecutionPhase::PlayingStarted);
        trace.push(1_000_000, &d1, 2, ExecutionPhase::PlayingStarted);
        trace.push(
            2_000_000,
            &d0,
            2,
            ExecutionPhase::ActionCollectionStarted { turn: 0 },
        );
        trace.push(
            2_000_000,
            &d1,
            2,
            ExecutionPhase::ActionCollectionStarted { turn: 0 },
        );
        trace.push(3_000_000, &d0, 2, ExecutionPhase::TurnCompleted { turn: 0 });
        trace.push(3_000_000, &d1, 2, ExecutionPhase::TurnCompleted { turn: 0 });
        trace.push(
            4_000_000,
            &d1,
            2,
            ExecutionPhase::ActionCollectionStarted { turn: 1 },
        );
        trace.push(
            10_000_000,
            &d0,
            2,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Finished,
            },
        );
        trace.push(10_000_000, &d0, 1, ExecutionPhase::PermitReleased);
        trace.push(
            10_000_000,
            &d2,
            1,
            ExecutionPhase::Queued {
                players: ["a".into(), "b".into()],
            },
        );
        trace.push(10_000_000, &d2, 2, ExecutionPhase::PermitAcquired);
        trace.push(11_000_000, &d2, 2, ExecutionPhase::PlayingStarted);
        trace.push(
            12_000_000,
            &d2,
            2,
            ExecutionPhase::ActionCollectionStarted { turn: 0 },
        );
        trace.push(
            13_000_000,
            &d2,
            2,
            ExecutionPhase::TurnCompleted { turn: 0 },
        );
        trace.push(
            14_000_000,
            &d2,
            2,
            ExecutionPhase::ActionCollectionStarted { turn: 1 },
        );
        trace.push(
            20_000_000,
            &d1,
            2,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Finished,
            },
        );
        trace.push(20_000_000, &d1, 1, ExecutionPhase::PermitReleased);
        trace.push(
            20_000_000,
            &d3,
            1,
            ExecutionPhase::Queued {
                players: ["a".into(), "b".into()],
            },
        );
        trace.push(20_000_000, &d3, 2, ExecutionPhase::PermitAcquired);
        trace.push(
            21_000_000,
            &d3,
            2,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Finished,
            },
        );
        trace.push(21_000_000, &d3, 1, ExecutionPhase::PermitReleased);
        trace.push(
            40_000_000,
            &d2,
            1,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Finished,
            },
        );
        trace.push(40_000_000, &d2, 0, ExecutionPhase::PermitReleased);
        trace.finish()
    }

    #[test]
    fn m0_reproduces_terminal_slot_throughput_and_final_partial_batch() {
        let mut events = Vec::new();
        for rep in 0..4 {
            let d = descriptor(u64::from(rep), rep, 0);
            events.push(event(
                u64::from(rep) * 4,
                0,
                d.clone(),
                0,
                ExecutionPhase::Queued {
                    players: ["a".into(), "b".into()],
                },
            ));
            events.push(event(
                u64::from(rep) * 4 + 1,
                if rep < 2 { 0 } else { 10_000_000 },
                d.clone(),
                2,
                ExecutionPhase::PermitAcquired,
            ));
            let terminal_at = if rep < 2 { 10_000_000 } else { 20_000_000 };
            events.push(event(
                u64::from(rep) * 4 + 2,
                terminal_at,
                d.clone(),
                2,
                ExecutionPhase::TerminalPersisted {
                    outcome: AttemptTerminal::Finished,
                },
            ));
            events.push(event(
                u64::from(rep) * 4 + 3,
                terminal_at,
                d,
                if rep % 2 == 0 { 1 } else { 0 },
                ExecutionPhase::PermitReleased,
            ));
        }
        let trace = TournamentTimingTrace {
            schema_version: TIMING_TRACE_SCHEMA_VERSION,
            dispatch_policy: DispatchPolicy::PairMajorV1,
            max_parallel: 2,
            max_failures_per_slot: 2,
            slots: (0..4).map(slot).collect(),
            events,
        };

        let points = replay_m0(&trace, 7).unwrap();
        let after_first_batch = points
            .iter()
            .find(|point| {
                point.at.micros == 10_000_000
                    && point.estimate.observed_terminal_slots == 2
                    && point.estimate.state == EtaState::Ready
            })
            .expect("ready after two terminal samples");
        assert_eq!(after_first_batch.estimate.remaining_slots, 2);
        assert_eq!(after_first_batch.estimate.seconds, Some(10));
        assert_eq!(points.last().unwrap().estimate.state, EtaState::Complete);
    }

    #[test]
    fn m1_uses_scheduler_shape_for_the_final_partial_batch() {
        let trace = final_partial_batch_trace();
        assert_eq!(replay_m1(&trace, 7), replay_m1(&trace, 999));
        assert_eq!(replay_m1p(&trace, 7), replay_m1p(&trace, 999));
        let comparison = compare_eta_models_walk_forward(&trace, 7).unwrap();
        let json = serde_json::to_string(&comparison).unwrap();
        let decoded: Vec<EtaModelComparisonPoint> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, comparison);
        let point = comparison
            .iter()
            .find(|point| {
                point.at.micros == 10_000_000
                    && point.m0.estimate.seconds == Some(5)
                    && point.m1.estimate.seconds == Some(10)
            })
            .expect("both candidates ready when the final lone attempt starts");

        assert_eq!(point.actual_remaining_seconds, 10);
        assert_eq!(point.m0.absolute_error_seconds, Some(5));
        assert_eq!(point.m1.absolute_error_seconds, Some(0));
        assert_eq!(point.m1p.estimate.seconds, Some(10));
        assert_eq!(point.m1p.absolute_error_seconds, Some(0));
    }

    #[test]
    fn phase_turn_residual_repairs_the_pinned_active_straggler_regression() {
        let trace = active_straggler_trace();
        let comparison = compare_eta_models_walk_forward(&trace, 11).unwrap();
        let point = comparison
            .iter()
            .find(|point| {
                point.at.micros == 20_000_000
                    && point.m0.estimate.seconds == Some(20)
                    && point.m1.estimate.seconds == Some(15)
            })
            .expect("both candidates ready with one aged and one fresh active attempt");

        assert_eq!(point.actual_remaining_seconds, 20);
        assert_eq!(point.m0.absolute_error_seconds, Some(0));
        assert_eq!(point.m1.signed_error_seconds, Some(-5));
        assert_eq!(point.m1.absolute_error_seconds, Some(5));

        let after_fresh_match = comparison
            .iter()
            .find(|point| {
                point.at.micros == 21_000_000
                    && point.m1.estimate.seconds == Some(1)
                    && point.m1p.estimate.seconds == Some(16)
            })
            .expect("M1P should retain the stalled match's turn-conditioned service");
        assert_eq!(after_fresh_match.actual_remaining_seconds, 19);
        assert_eq!(after_fresh_match.m1.absolute_error_seconds, Some(18));
        assert_eq!(after_fresh_match.m1p.signed_error_seconds, Some(-3));
        assert_eq!(after_fresh_match.m1p.absolute_error_seconds, Some(3));

        let mut slower_future = trace.clone();
        for event in &mut slower_future.events {
            if event.observed_at.micros > 20_000_000 {
                event.observed_at.micros += 10_000_000;
            }
        }
        let original_prefix: Vec<_> = replay_m1(&trace, 11)
            .unwrap()
            .into_iter()
            .filter(|point| point.at.micros <= 20_000_000)
            .collect();
        let changed_prefix: Vec<_> = replay_m1(&slower_future, 11)
            .unwrap()
            .into_iter()
            .filter(|point| point.at.micros <= 20_000_000)
            .collect();
        assert_eq!(
            original_prefix, changed_prefix,
            "future timing must not leak"
        );

        let original_m1p_prefix: Vec<_> = replay_m1p(&trace, 11)
            .unwrap()
            .into_iter()
            .filter(|point| point.at.micros <= 20_000_000)
            .collect();
        let changed_m1p_prefix: Vec<_> = replay_m1p(&slower_future, 11)
            .unwrap()
            .into_iter()
            .filter(|point| point.at.micros <= 20_000_000)
            .collect();
        assert_eq!(
            original_m1p_prefix, changed_m1p_prefix,
            "future timing must not leak into M1P"
        );
    }

    #[test]
    fn m1_counts_a_failed_attempt_duration_and_the_required_retry() {
        let first = descriptor(0, 0, 0);
        let retry = descriptor(1, 0, 1);
        let mut synthetic = SyntheticTrace::new(1, 1);
        synthetic.push(
            0,
            &first,
            0,
            ExecutionPhase::Queued {
                players: ["a".into(), "b".into()],
            },
        );
        synthetic.push(0, &first, 1, ExecutionPhase::PermitAcquired);
        synthetic.push(
            5_000_000,
            &first,
            1,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Failed {
                    durable_record: true,
                },
            },
        );
        synthetic.push(5_000_000, &first, 0, ExecutionPhase::PermitReleased);
        synthetic.push(
            5_000_000,
            &retry,
            0,
            ExecutionPhase::Queued {
                players: ["a".into(), "b".into()],
            },
        );
        synthetic.push(5_000_000, &retry, 1, ExecutionPhase::PermitAcquired);
        synthetic.push(
            10_000_000,
            &retry,
            1,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Finished,
            },
        );
        synthetic.push(10_000_000, &retry, 0, ExecutionPhase::PermitReleased);
        let mut trace = synthetic.finish();
        trace.max_failures_per_slot = 2;

        let comparison = compare_eta_models_walk_forward(&trace, 0).unwrap();
        let after_failed_attempt = comparison
            .iter()
            .find(|point| {
                point.at.micros == 5_000_000
                    && point.m1.estimate.state == EtaState::Ready
                    && point.m1.estimate.seconds == Some(5)
            })
            .expect("failed attempt duration should seed the required retry");
        assert_eq!(after_failed_attempt.actual_remaining_seconds, 5);
        assert_eq!(after_failed_attempt.m1.absolute_error_seconds, Some(0));
    }

    #[test]
    fn comparison_rejects_incomplete_or_impossible_traces() {
        let d = descriptor(0, 0, 0);
        let incomplete = TournamentTimingTrace {
            schema_version: TIMING_TRACE_SCHEMA_VERSION,
            dispatch_policy: DispatchPolicy::PairMajorV1,
            max_parallel: 1,
            max_failures_per_slot: 1,
            slots: vec![slot(0)],
            events: vec![event(
                0,
                0,
                d.clone(),
                0,
                ExecutionPhase::Queued {
                    players: ["a".into(), "b".into()],
                },
            )],
        };
        assert_eq!(
            compare_eta_models_walk_forward(&incomplete, 0),
            Err(EtaTraceError::IncompleteTrace {
                observed_terminal_slots: 0,
                total_slots: 1,
            })
        );

        let impossible = TournamentTimingTrace {
            events: vec![event(0, 0, d, 2, ExecutionPhase::PermitAcquired)],
            ..incomplete
        };
        assert_eq!(
            replay_m1(&impossible, 0),
            Err(EtaTraceError::ConcurrencyExceedsLimit {
                observed: 2,
                max_parallel: 1,
            })
        );
    }

    #[test]
    fn durable_failures_only_finish_a_slot_at_the_retry_budget() {
        let first = descriptor(1, 0, 0);
        let retry = descriptor(2, 0, 1);
        let trace = TournamentTimingTrace {
            schema_version: TIMING_TRACE_SCHEMA_VERSION,
            dispatch_policy: DispatchPolicy::PairMajorV1,
            max_parallel: 1,
            max_failures_per_slot: 2,
            slots: vec![slot(0)],
            events: vec![
                event(
                    0,
                    0,
                    first.clone(),
                    0,
                    ExecutionPhase::Queued {
                        players: ["a".into(), "b".into()],
                    },
                ),
                event(
                    1,
                    1_000_000,
                    first,
                    1,
                    ExecutionPhase::TerminalPersisted {
                        outcome: AttemptTerminal::Failed {
                            durable_record: true,
                        },
                    },
                ),
                event(
                    2,
                    1_000_000,
                    retry.clone(),
                    0,
                    ExecutionPhase::Queued {
                        players: ["a".into(), "b".into()],
                    },
                ),
                event(
                    3,
                    2_000_000,
                    retry,
                    1,
                    ExecutionPhase::TerminalPersisted {
                        outcome: AttemptTerminal::Failed {
                            durable_record: true,
                        },
                    },
                ),
            ],
        };

        let points = replay_m0(&trace, 0).unwrap();
        assert_eq!(points[1].estimate.observed_terminal_slots, 0);
        assert_eq!(points[3].estimate.state, EtaState::Complete);
    }

    #[test]
    fn replay_is_seed_independent_and_sorts_host_timestamps_before_sequence() {
        let d = descriptor(1, 0, 0);
        let trace = TournamentTimingTrace {
            schema_version: TIMING_TRACE_SCHEMA_VERSION,
            dispatch_policy: DispatchPolicy::PairMajorV1,
            max_parallel: 1,
            max_failures_per_slot: 1,
            slots: vec![slot(0)],
            events: vec![
                event(
                    0,
                    2_000_000,
                    d.clone(),
                    1,
                    ExecutionPhase::TerminalPersisted {
                        outcome: AttemptTerminal::Finished,
                    },
                ),
                event(
                    1,
                    0,
                    d,
                    0,
                    ExecutionPhase::Queued {
                        players: ["a".into(), "b".into()],
                    },
                ),
            ],
        };

        assert_eq!(replay_m0(&trace, 1), replay_m0(&trace, 999));
        assert!(replay_m0(&trace, 1).is_ok());
    }

    #[test]
    fn non_durable_failure_does_not_become_terminal_evidence() {
        let d = descriptor(1, 0, 0);
        let trace = TournamentTimingTrace {
            schema_version: TIMING_TRACE_SCHEMA_VERSION,
            dispatch_policy: DispatchPolicy::PairMajorV1,
            max_parallel: 1,
            max_failures_per_slot: 1,
            slots: vec![slot(0)],
            events: vec![
                event(
                    0,
                    0,
                    d.clone(),
                    0,
                    ExecutionPhase::Queued {
                        players: ["a".into(), "b".into()],
                    },
                ),
                event(
                    1,
                    2_000_000,
                    d,
                    0,
                    ExecutionPhase::TerminalProduced {
                        outcome: AttemptTerminal::Failed {
                            durable_record: false,
                        },
                    },
                ),
            ],
        };

        let points = replay_m0(&trace, 0).unwrap();
        assert_eq!(points.last().unwrap().estimate.observed_terminal_slots, 0);
    }

    #[test]
    fn schedule_snapshot_separates_active_remaining_and_terminal_slots() {
        let d0 = descriptor(1, 0, 0);
        let d1 = descriptor(2, 1, 0);
        let trace = TournamentTimingTrace {
            schema_version: TIMING_TRACE_SCHEMA_VERSION,
            dispatch_policy: DispatchPolicy::PairMajorV1,
            max_parallel: 2,
            max_failures_per_slot: 1,
            slots: (0..3).map(slot).collect(),
            events: vec![
                event(
                    0,
                    0,
                    d0.clone(),
                    0,
                    ExecutionPhase::Queued {
                        players: ["a".into(), "b".into()],
                    },
                ),
                event(
                    1,
                    1_000_000,
                    d0,
                    1,
                    ExecutionPhase::TerminalPersisted {
                        outcome: AttemptTerminal::Finished,
                    },
                ),
                event(
                    2,
                    1_000_000,
                    d1.clone(),
                    1,
                    ExecutionPhase::Queued {
                        players: ["a".into(), "b".into()],
                    },
                ),
                event(3, 1_500_000, d1, 1, ExecutionPhase::PlayingStarted),
            ],
        };

        let snapshot = schedule_snapshot(&trace).unwrap();
        assert_eq!(snapshot.terminal, vec![slot(0)]);
        assert_eq!(snapshot.active.len(), 1);
        assert_eq!(snapshot.active[0].descriptor.repetition_index, 1);
        assert_eq!(snapshot.remaining, vec![slot(2)]);
    }

    #[test]
    fn produced_durable_terminal_stays_active_until_persistence() {
        let d = descriptor(1, 0, 0);
        let mut trace = TournamentTimingTrace {
            schema_version: TIMING_TRACE_SCHEMA_VERSION,
            dispatch_policy: DispatchPolicy::PairMajorV1,
            max_parallel: 1,
            max_failures_per_slot: 1,
            slots: vec![slot(0)],
            events: vec![
                event(
                    0,
                    0,
                    d.clone(),
                    0,
                    ExecutionPhase::Queued {
                        players: ["a".into(), "b".into()],
                    },
                ),
                event(
                    1,
                    1_000_000,
                    d.clone(),
                    1,
                    ExecutionPhase::TerminalProduced {
                        outcome: AttemptTerminal::Finished,
                    },
                ),
            ],
        };

        let pending = schedule_snapshot(&trace).unwrap();
        assert_eq!(pending.active.len(), 1);
        assert!(pending.remaining.is_empty());
        assert!(pending.terminal.is_empty());

        trace.events.push(event(
            2,
            2_000_000,
            d,
            1,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Finished,
            },
        ));
        let persisted = schedule_snapshot(&trace).unwrap();
        assert!(persisted.active.is_empty());
        assert!(persisted.remaining.is_empty());
        assert_eq!(persisted.terminal, vec![slot(0)]);
    }

    #[test]
    fn timing_trace_json_is_a_versioned_replay_artifact() {
        let trace = TournamentTimingTrace {
            schema_version: TIMING_TRACE_SCHEMA_VERSION,
            dispatch_policy: DispatchPolicy::PairMajorV1,
            max_parallel: 1,
            max_failures_per_slot: 1,
            slots: vec![slot(0)],
            events: vec![event(
                0,
                0,
                descriptor(1, 0, 0),
                0,
                ExecutionPhase::Queued {
                    players: ["a".into(), "b".into()],
                },
            )],
        };
        let json = serde_json::to_string(&trace).unwrap();
        let decoded: TournamentTimingTrace = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, trace);

        let mut unsupported = decoded;
        unsupported.schema_version += 1;
        assert_eq!(
            replay_m0(&unsupported, 0),
            Err(EtaTraceError::UnsupportedSchema {
                expected: TIMING_TRACE_SCHEMA_VERSION,
                got: TIMING_TRACE_SCHEMA_VERSION + 1,
            })
        );
    }
}
