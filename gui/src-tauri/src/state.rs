use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub enum MatchPhase {
    Idle,
    Running {
        match_id: u32,
        cancel: CancellationToken,
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
