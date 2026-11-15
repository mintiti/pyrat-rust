//! Lossless, monotonic execution observations for duration modelling.
//!
//! The ordinary lifecycle stream answers what happened to a match.  This
//! stream records when the execution machinery crossed the phase boundaries
//! that determine wall time.  Observers are optional and synchronous: the
//! default is a no-op, while eval sessions install an in-memory recorder.

use serde::{Deserialize, Serialize};

use crate::descriptor::Descriptor;
use crate::outcome::{FailureReason, TimeoutPhase};

use pyrat_host::match_host::MatchResult;
use pyrat_host::wire::{GameResult, Player};

/// Microseconds since one orchestrator was created.
///
/// A relative monotonic value is replayable and immune to wall-clock jumps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MonotonicTime {
    pub micros: u64,
}

impl MonotonicTime {
    pub const ZERO: Self = Self { micros: 0 };

    pub const fn from_micros(micros: u64) -> Self {
        Self { micros }
    }
}

/// Terminal classification needed to replay slot progress honestly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptTerminal {
    Finished,
    Failed { durable_record: bool },
}

/// Compact terminal evidence used by schedule-policy experiments. Exact
/// failure strings remain in durable attempts; this trace shape retains only
/// the outcome dimensions needed to compare policies without making timing
/// fixtures depend on user-facing prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttemptDetail {
    Finished {
        result: TimingGameResult,
        /// PyRat scores advance in half-point increments. Integer half-points
        /// keep the timing stream `Eq` and JSON-stable.
        player1_half_points: u32,
        player2_half_points: u32,
        turns_played: u16,
    },
    Failed {
        reason: TimingFailureReason,
    },
}

impl AttemptDetail {
    pub(crate) fn from_result(result: &MatchResult) -> Self {
        let game_result = match result.result {
            GameResult::Player1 => TimingGameResult::Player1,
            GameResult::Player2 => TimingGameResult::Player2,
            GameResult::Draw => TimingGameResult::Draw,
            _ => TimingGameResult::Unknown,
        };
        Self::Finished {
            result: game_result,
            player1_half_points: score_half_points(result.player1_score),
            player2_half_points: score_half_points(result.player2_score),
            turns_played: result.turns_played,
        }
    }

    pub(crate) fn from_failure(reason: &FailureReason) -> Self {
        Self::Failed {
            reason: TimingFailureReason::from(reason),
        }
    }
}

fn score_half_points(score: f32) -> u32 {
    (score.max(0.0) * 2.0).round() as u32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingGameResult {
    Player1,
    Player2,
    Draw,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum TimingFailureReason {
    SpawnFailed,
    HandshakeTimeout,
    Disconnected {
        player: TimingPlayer,
    },
    Timeout {
        player: TimingPlayer,
        phase: TimingTimeoutPhase,
    },
    ProtocolError,
    Panic,
    Cancelled,
    SinkFlushError,
    Internal,
}

impl From<&FailureReason> for TimingFailureReason {
    fn from(reason: &FailureReason) -> Self {
        match reason {
            FailureReason::SpawnFailed => Self::SpawnFailed,
            FailureReason::HandshakeTimeout => Self::HandshakeTimeout,
            FailureReason::Disconnected(player) => Self::Disconnected {
                player: TimingPlayer::from(*player),
            },
            FailureReason::Timeout { slot, phase } => Self::Timeout {
                player: TimingPlayer::from(*slot),
                phase: TimingTimeoutPhase::from(*phase),
            },
            FailureReason::ProtocolError(_) => Self::ProtocolError,
            FailureReason::Panic => Self::Panic,
            FailureReason::Cancelled => Self::Cancelled,
            FailureReason::SinkFlushError(_) => Self::SinkFlushError,
            FailureReason::Internal(_) => Self::Internal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingPlayer {
    Player1,
    Player2,
    Unknown,
}

impl From<Player> for TimingPlayer {
    fn from(player: Player) -> Self {
        match player {
            Player::Player1 => Self::Player1,
            Player::Player2 => Self::Player2,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingTimeoutPhase {
    Setup,
    Preprocessing,
    Sync,
    Move,
}

impl From<TimeoutPhase> for TimingTimeoutPhase {
    fn from(phase: TimeoutPhase) -> Self {
        match phase {
            TimeoutPhase::Setup => Self::Setup,
            TimeoutPhase::Preprocessing => Self::Preprocessing,
            TimeoutPhase::Sync => Self::Sync,
            TimeoutPhase::Move => Self::Move,
        }
    }
}

/// One owned execution phase.  Player ids on `Queued` are in engine-seat
/// order, so a replay can model which bots shared the machine concurrently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExecutionPhase {
    Queued {
        players: [String; 2],
    },
    PermitAcquired,
    LaunchStarted,
    HandshakeCompleted {
        players: [String; 2],
    },
    ConfigureStarted,
    PreprocessingStarted,
    SetupCompleted,
    PlayingStarted,
    ActionCollectionStarted {
        turn: u16,
    },
    ActionsCollected {
        turn: u16,
    },
    TurnCompleted {
        turn: u16,
    },
    TerminalProduced {
        outcome: AttemptTerminal,
    },
    /// Outcome/diagnostic detail emitted beside `TerminalProduced`. Keeping
    /// it as an additive phase preserves schema-v1 timing fixtures and leaves
    /// ETA terminal semantics unchanged.
    TerminalDetail {
        detail: AttemptDetail,
    },
    TerminalPersisted {
        outcome: AttemptTerminal,
    },
    TerminalPersistenceFailed,
    PermitReleased,
}

/// A descriptor-bearing observation suitable for an in-memory trace and
/// later JSON fixtures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTimingEvent<D> {
    /// Tie-breaker assigned when the orchestrator receives the observation.
    /// Replay orders by `(observed_at, sequence)`: host-owned timestamps can
    /// legitimately arrive after a sink delay even though they happened first.
    pub sequence: u64,
    pub observed_at: MonotonicTime,
    pub active_concurrency: usize,
    pub descriptor: D,
    pub phase: ExecutionPhase,
}

/// Optional consumer of the execution timing stream.
pub trait TimingObserver<D: Descriptor>: Send + Sync {
    fn observe(&self, event: ExecutionTimingEvent<D>);
}

/// Default observer used by callers that do not request duration evidence.
#[derive(Debug, Default)]
pub struct NoOpTimingObserver;

impl<D: Descriptor> TimingObserver<D> for NoOpTimingObserver {
    fn observe(&self, _event: ExecutionTimingEvent<D>) {}
}
