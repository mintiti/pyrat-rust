//! Terminal values that flow out of the orchestrator: success and failure.
//!
//! Both shapes carry the descriptor verbatim so sinks can correlate with
//! the durable record they wrote at submit time. `durable_record` on
//! `MatchFailure` distinguishes "store has a row for this failure" (the
//! normal failure path) from "match was lost without a durable row"
//! (kill-9 mid-match, or required-sink terminal flush failure).

use std::time::SystemTime;

use pyrat_host::match_host::MatchResult;
use pyrat_host::player::PlayerIdentity;

use crate::descriptor::Descriptor;

/// Result of a match that completed cleanly.
#[derive(Debug, Clone)]
pub struct MatchOutcome<D: Descriptor> {
    pub descriptor: D,
    pub started_at: SystemTime,
    pub finished_at: SystemTime,
    pub result: MatchResult,
    pub players: [PlayerIdentity; 2],
}

/// Result of a match that failed.
///
/// `players` is `Option` because a failure can occur before both players
/// completed handshake (e.g. spawn failure, handshake timeout). `started_at`
/// is similarly optional for failures detected before the playing phase
/// began.
#[derive(Debug, Clone)]
pub struct MatchFailure<D: Descriptor> {
    pub descriptor: D,
    pub started_at: Option<SystemTime>,
    pub failed_at: SystemTime,
    pub reason: FailureReason,
    pub players: Option<[PlayerIdentity; 2]>,
    /// True when the store has a row for this failure (normal failure path).
    /// False for kill-9 mid-match or required-sink terminal flush failure —
    /// resume must re-issue at the same `attempt_index`.
    pub durable_record: bool,
}

/// Why a match failed. Operational categories, not user-facing messages.
#[derive(Debug, Clone)]
pub enum FailureReason {
    SpawnFailed,
    HandshakeTimeout,
    Disconnected,
    ProtocolError,
    Panic,
    Cancelled,
    SinkFlushError,
    Internal(String),
}
