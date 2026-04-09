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

pub struct AppState {
    pub match_phase: Arc<Mutex<MatchPhase>>,
    pub next_match_id: AtomicU32,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            match_phase: Arc::new(Mutex::new(MatchPhase::Idle)),
            next_match_id: AtomicU32::new(0),
        }
    }
}
