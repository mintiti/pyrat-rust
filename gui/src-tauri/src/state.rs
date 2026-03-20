use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use pyrat_host::wire::Direction as WireDirection;

// ── Analysis channel types ──────────────────────────

pub enum AnalysisCmd {
    StartTurn { duration_ms: u64 },
    StopCollect,
    Advance { actions: Option<[WireDirection; 2]> },
}

#[allow(dead_code)] // Error variant reserved for future use by the analysis loop
pub enum AnalysisResp {
    TurnStarted,
    Actions {
        p1: WireDirection,
        p2: WireDirection,
    },
    Advanced {
        p1: WireDirection,
        p2: WireDirection,
        game_over: bool,
    },
    Error(String),
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
