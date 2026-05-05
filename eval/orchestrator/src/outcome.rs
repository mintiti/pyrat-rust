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
use pyrat_host::wire::Player as PlayerSlot;

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
    /// False for kill-9 mid-match or required-sink terminal flush failure.
    /// In the false case, resume must re-issue at the same `attempt_index`.
    pub durable_record: bool,
}

/// Why a match failed. Operational categories, not user-facing messages.
///
/// `ProtocolError`, `Disconnected`, and `SinkFlushError` carry payloads so a
/// failed-tournament forensic pass has enough context to triage without
/// reaching back into per-match logs: the underlying error string for
/// protocol faults, the player slot for clean disconnects, the propagated
/// sink error string for flush failures.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum FailureReason {
    SpawnFailed,
    HandshakeTimeout,
    /// A player closed transport-cleanly while Match needed it. The slot
    /// identifies which one.
    Disconnected(PlayerSlot),
    /// Protocol-layer fault (timeout, hash mismatch, malformed message).
    /// Payload is the underlying `MatchError`/`PlayerError` rendered via
    /// `Display` at the fault site, enough to triage without reaching back
    /// into per-match logs.
    ProtocolError(String),
    Panic,
    Cancelled,
    /// A `Required` sink errored on a terminal callback (or on
    /// `on_match_event` while a match was running). Payload is the
    /// `SinkError` rendered via `Display`.
    SinkFlushError(String),
    Internal(String),
}
