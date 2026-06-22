use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, Mutex};
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
    Starting {
        cancel: CancellationToken,
    },
    Running {
        tournament_id: i64,
        cancel: CancellationToken,
        handle: JoinHandle<()>,
    },
}

pub struct AppState {
    pub match_phase: Arc<Mutex<MatchPhase>>,
    pub tournament_phase: Arc<Mutex<TournamentPhase>>,
    pub next_match_id: AtomicU32,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            match_phase: Arc::new(Mutex::new(MatchPhase::Idle)),
            tournament_phase: Arc::new(Mutex::new(TournamentPhase::Idle)),
            next_match_id: AtomicU32::new(0),
        }
    }
}
