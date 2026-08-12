use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use parking_lot::Mutex as SyncMutex;
use tokio::sync::{mpsc, oneshot, watch, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use pyrat::Direction;

use crate::commands::AnalysisPosition;

// ── Analysis channel types ──────────────────────────

pub enum AnalysisCmd {
    StartTurn { position: Option<AnalysisPosition> },
    StopTurn,
    Advance { actions: Option<[Direction; 2]> },
}

pub enum AnalysisResp {
    TurnStarted,
    Actions {
        p1: Direction,
        p2: Direction,
    },
    Advanced {
        p1: Direction,
        p2: Direction,
        game_over: bool,
    },
}

pub type AnalysisTx = mpsc::Sender<(AnalysisCmd, oneshot::Sender<AnalysisResp>)>;
pub type AnalysisRx = mpsc::Receiver<(AnalysisCmd, oneshot::Sender<AnalysisResp>)>;

// ── App state ───────────────────────────────────────

pub enum MatchPhase {
    Idle,
    Running {
        match_id: u32,
        cancel: CancellationToken,
        handle: JoinHandle<()>,
        cmd_tx: Option<AnalysisTx>,
    },
}

/// Tournament lifecycle, kept in a slot separate from `match_phase` so a
/// tournament can run in the background while Play/Analysis is active. The
/// two never share resources: per-match TCP listeners bind ephemeral ports,
/// so the only shared cost is process load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TournamentStopCause {
    User,
    AppShutdown,
}

#[derive(Clone)]
pub struct TournamentControl {
    cancel: CancellationToken,
    stop_cause: Arc<SyncMutex<Option<TournamentStopCause>>>,
}

impl TournamentControl {
    pub fn new() -> Self {
        Self {
            cancel: CancellationToken::new(),
            stop_cause: Arc::new(SyncMutex::new(None)),
        }
    }

    pub fn token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn request_stop(&self, cause: TournamentStopCause) {
        let mut current = self.stop_cause.lock();
        if current.is_none()
            || matches!(
                (*current, cause),
                (
                    Some(TournamentStopCause::User),
                    TournamentStopCause::AppShutdown
                )
            )
        {
            *current = Some(cause);
        }
        self.cancel.cancel();
    }

    pub fn stop_cause(&self) -> Option<TournamentStopCause> {
        *self.stop_cause.lock()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentSupervisorCompletion {
    pub generation: u64,
    pub tournament_id: Option<i64>,
    pub result: Result<(), String>,
}

#[derive(Clone)]
pub struct TournamentLease {
    pub generation: u64,
    pub tournament_id: Option<i64>,
    pub control: TournamentControl,
    pub completion: watch::Receiver<Option<TournamentSupervisorCompletion>>,
}

impl TournamentLease {
    pub fn with_tournament_id(&self, tournament_id: i64) -> Self {
        Self {
            tournament_id: Some(tournament_id),
            ..self.clone()
        }
    }
}

pub enum TournamentPhase {
    Idle,
    /// A start is in flight: the slot is reserved under the phase lock before
    /// the async create/open-store/spawn work, then promoted to `Running`.
    /// Without this, two concurrent `start_tournament` calls (a second window,
    /// a slow-launch retry) both pass an `Idle` check while the lock is
    /// released across the await, and the second orphans the first's runner.
    ///
    /// Carries the `cancel` token from the moment the slot is reserved (not
    /// only once `Running`), so a `stop_tournament` landing during the
    /// pre-tournament bot warmup — which can cold-build for tens of seconds —
    /// is honored immediately instead of waiting for the launch to return.
    /// The same token is reused as the runner's token on promotion to
    /// `Running`, so the cancel signal spans the whole start lifetime.
    Starting(TournamentLease),
    Running(TournamentLease),
    /// Stop has been requested, but this generation still owns the slot until
    /// its supervisor persists a terminal outcome and fully unwinds.
    Stopping(TournamentLease),
}

impl TournamentPhase {
    pub fn lease(&self) -> Option<&TournamentLease> {
        match self {
            Self::Idle => None,
            Self::Starting(lease) | Self::Running(lease) | Self::Stopping(lease) => Some(lease),
        }
    }

    pub fn record_tournament_id(&mut self, generation: u64, tournament_id: i64) -> bool {
        let Some(lease) = self.lease() else {
            return false;
        };
        if lease.generation != generation {
            return false;
        }
        let lease = lease.with_tournament_id(tournament_id);
        *self = match self {
            Self::Starting(_) => Self::Starting(lease),
            Self::Running(_) => Self::Running(lease),
            Self::Stopping(_) => Self::Stopping(lease),
            Self::Idle => unreachable!("idle was rejected above"),
        };
        true
    }

    pub fn promote_running(&mut self, generation: u64) -> bool {
        let Self::Starting(lease) = self else {
            return false;
        };
        if lease.generation != generation || lease.tournament_id.is_none() {
            return false;
        }
        *self = Self::Running(lease.clone());
        true
    }

    pub fn request_stop(&mut self, cause: TournamentStopCause) -> Option<TournamentLease> {
        let lease = self.lease()?.clone();
        lease.control.request_stop(cause);
        if !matches!(self, Self::Stopping(_)) {
            *self = Self::Stopping(lease.clone());
        }
        Some(lease)
    }

    pub fn clear_if_generation(&mut self, generation: u64) -> bool {
        if self.lease().map(|lease| lease.generation) != Some(generation) {
            return false;
        }
        *self = Self::Idle;
        true
    }
}

pub struct AppState {
    pub match_phase: Arc<Mutex<MatchPhase>>,
    pub tournament_phase: Arc<Mutex<TournamentPhase>>,
    pub next_tournament_generation: AtomicU64,
    pub next_match_id: AtomicU32,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            match_phase: Arc::new(Mutex::new(MatchPhase::Idle)),
            tournament_phase: Arc::new(Mutex::new(TournamentPhase::Idle)),
            next_tournament_generation: AtomicU64::new(1),
            next_match_id: AtomicU32::new(0),
        }
    }
}
