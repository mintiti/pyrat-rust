//! Top-level orchestrator errors.
//!
//! Two layers:
//! - [`OrchestratorError`]: what callers of `submit` / `shutdown` see.
//! - [`OrchestratorInternalError`]: what the run-loop emits before exiting
//!   uncleanly. Surfaces through the join handle.

/// Failures observed by callers of the public [`Orchestrator`] API.
///
/// [`Orchestrator`]: crate::executor::Orchestrator
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    /// The orchestrator's run-loop has exited (cancellation, driver-drop,
    /// or panic). New submissions can't be accepted.
    #[error("orchestrator is shut down")]
    ShutDown,
}

/// Internal failure modes the run-loop can hit. Surfaced through the
/// orchestrator's join handle on shutdown. Mostly forensic, since the
/// public API treats any of these as `ShutDown` to callers.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum OrchestratorInternalError {
    /// The owning driver dropped its [`mpsc::Receiver<DriverEvent>`]. The
    /// run-loop cannot continue: terminal events with no consumer mean
    /// matches whose outcomes nobody records.
    ///
    /// [`mpsc::Receiver<DriverEvent>`]: tokio::sync::mpsc::Receiver
    #[error("driver receiver dropped, orchestrator cannot continue")]
    DriverDropped,
}
