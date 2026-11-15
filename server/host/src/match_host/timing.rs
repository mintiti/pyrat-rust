//! Monotonic phase observations for one hosted match.
//!
//! These events are deliberately separate from [`MatchEvent`](super::MatchEvent):
//! match events are product/replay facts, while this stream is measurement
//! instrumentation.  Timestamps are taken at the phase owner, before an
//! orchestrator, sink, or UI can add queueing delay.

use std::time::Instant;

use tokio::sync::mpsc;

/// A host-owned phase boundary that can contribute to duration models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatchTimingPhase {
    ConfigureStarted,
    PreprocessingStarted,
    SetupCompleted,
    PlayingStarted,
    ActionCollectionStarted { turn: u16 },
    ActionsCollected { turn: u16 },
    TurnCompleted { turn: u16 },
}

/// One phase observation stamped from the host's monotonic clock.
#[derive(Debug, Clone, Copy)]
pub struct MatchTimingEvent {
    pub observed_at: Instant,
    pub phase: MatchTimingPhase,
}

pub(crate) fn emit(tx: Option<&mpsc::UnboundedSender<MatchTimingEvent>>, phase: MatchTimingPhase) {
    if let Some(tx) = tx {
        // Timing observation is optional instrumentation.  A dropped
        // consumer must never change match behavior or add warning noise.
        let _ = tx.send(MatchTimingEvent {
            observed_at: Instant::now(),
            phase,
        });
    }
}
