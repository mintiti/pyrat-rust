use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::AbortHandle;

pub enum MatchPhase {
    Idle,
    Running { abort_handle: AbortHandle },
}

pub struct AppState {
    pub match_phase: Arc<Mutex<MatchPhase>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            match_phase: Arc::new(Mutex::new(MatchPhase::Idle)),
        }
    }
}
