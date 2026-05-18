//! Type-conversion glue between the orchestrator/host shapes and the store
//! shapes. Centralised here so the smear-risk fields (timestamps, scores,
//! failure reasons, game-config flattening) live in one place.

use std::time::{SystemTime, UNIX_EPOCH};

use pyrat::game::builder::{CheeseStrategy, GameConfig, MazeStrategy};
use pyrat_eval_store::{
    AttemptKey, GameConfigRecord, NewAttempt, NewAttemptOutcome, RecordAttemptError, TournamentId,
};
use pyrat_orchestrator::{FailureReason, MatchFailure, MatchOutcome};

use crate::descriptor::EvalMatchDescriptor;

/// Errors when an orchestrator-side value can't be projected to a store
/// shape. Currently the only non-trivial case is `GameConfig` carrying a
/// strategy variant the store schema cannot represent (custom walls or
/// custom cheese positions).
#[derive(Debug, thiserror::Error)]
pub enum MappingError {
    #[error("game config uses fixed-walls maze strategy; eval-store schema only represents random mazes")]
    FixedMazeUnsupported,

    #[error("game config uses fixed-positions cheese strategy; eval-store schema only represents random cheese")]
    FixedCheeseUnsupported,

    #[error("record_attempt failed: {0}")]
    RecordAttempt(#[from] RecordAttemptError),
}

/// Project a runtime `GameConfig` to its durable record shape.
///
/// The store schema only represents random-mazes + random-cheese
/// configurations; fixed (custom) layouts return an error. Custom layouts
/// can be supported later by extending `GameConfigRecord` (e.g. a
/// `wall_layout_hash` field), out of scope for v1.
pub fn game_config_to_record(cfg: &GameConfig) -> Result<GameConfigRecord, MappingError> {
    let (wall_density, mud_density, mud_range, connected, symmetric) = match cfg.maze() {
        MazeStrategy::Random(p) => (
            f64::from(p.wall_density),
            f64::from(p.mud_density),
            u32::from(p.mud_range),
            p.connected,
            p.symmetric,
        ),
        MazeStrategy::Fixed { .. } => return Err(MappingError::FixedMazeUnsupported),
    };
    let (cheese_count, cheese_symmetric) = match cfg.cheese() {
        CheeseStrategy::Random { count, symmetric } => (u32::from(*count), *symmetric),
        CheeseStrategy::Fixed(_) => return Err(MappingError::FixedCheeseUnsupported),
    };
    Ok(GameConfigRecord {
        width: u32::from(cfg.width()),
        height: u32::from(cfg.height()),
        max_turns: u32::from(cfg.max_turns()),
        wall_density,
        mud_density,
        mud_range,
        connected,
        symmetric,
        cheese_count,
        cheese_symmetric,
    })
}

/// Project a successful `MatchOutcome` to a `NewAttempt` ready for
/// `record_attempt`.
pub fn outcome_to_new_attempt(outcome: &MatchOutcome<EvalMatchDescriptor>) -> NewAttempt {
    let desc = &outcome.descriptor;
    NewAttempt {
        key: attempt_key(desc),
        finished_at: format_sqlite_datetime(outcome.finished_at),
        outcome: NewAttemptOutcome::Success {
            player1_score: f64::from(outcome.result.player1_score),
            player2_score: f64::from(outcome.result.player2_score),
            turns: u32::from(outcome.result.turns_played),
            started_at: format_sqlite_datetime(outcome.started_at),
        },
    }
}

/// Project a `MatchFailure` to a `NewAttempt`. Caller is responsible for
/// only invoking this when `failure.durable_record == true`.
pub fn failure_to_new_attempt(failure: &MatchFailure<EvalMatchDescriptor>) -> NewAttempt {
    let desc = &failure.descriptor;
    NewAttempt {
        key: attempt_key(desc),
        finished_at: format_sqlite_datetime(failure.failed_at),
        outcome: NewAttemptOutcome::Failure {
            failure_reason: failure_reason_string(&failure.reason),
            started_at: failure.started_at.map(format_sqlite_datetime),
        },
    }
}

fn attempt_key(desc: &EvalMatchDescriptor) -> AttemptKey {
    AttemptKey {
        tournament_id: desc.tournament_id,
        game_config_id: desc.game_config_id.clone(),
        player1_id: desc.player1_id.clone(),
        player2_id: desc.player2_id.clone(),
        seed: desc.seed,
        repetition_index: desc.repetition_index,
        attempt_index: desc.attempt_index,
    }
}

/// Stable string label for a `FailureReason`. Stored in `match_attempts.failure_reason`.
///
/// Format: `"<category>"` for unit variants, `"<category>: <payload>"` for
/// payload-bearing ones. Stable across releases — planners may grep this
/// column to triage.
pub fn failure_reason_string(reason: &FailureReason) -> String {
    match reason {
        FailureReason::SpawnFailed => "spawn_failed".into(),
        FailureReason::HandshakeTimeout => "handshake_timeout".into(),
        FailureReason::Disconnected(slot) => format!("disconnected: {slot:?}"),
        FailureReason::ProtocolError(s) => format!("protocol_error: {s}"),
        FailureReason::Panic => "panic".into(),
        FailureReason::Cancelled => "cancelled".into(),
        FailureReason::SinkFlushError(s) => format!("sink_flush_error: {s}"),
        FailureReason::Internal(s) => format!("internal: {s}"),
        // FailureReason is `#[non_exhaustive]`. Future variants land here
        // as a Debug repr (rather than a flat "unknown") so two new
        // variants stay distinguishable for triage. The string is less
        // stable to grep against — accept that tradeoff in exchange for
        // preserving variant identity.
        _ => format!("unknown: {reason:?}"),
    }
}

/// SQLite-friendly UTC datetime: `"YYYY-MM-DD HH:MM:SS"`.
///
/// Mirrors what `datetime('now')` produces server-side. Computed inline
/// (Howard Hinnant's days→date algorithm) to avoid pulling in `time` /
/// `chrono`. Pre-1970 timestamps clamp to UNIX_EPOCH.
pub fn format_sqlite_datetime(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let secs_today = secs % 86_400;
    let hour = secs_today / 3600;
    let minute = (secs_today % 3600) / 60;
    let second = secs_today % 60;
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

/// Howard Hinnant's `civil_from_days` for 1970-01-01-based day counts.
/// <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>
fn days_to_ymd(days: u64) -> (i64, u32, u32) {
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y_within = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y_within + 1 } else { y_within };
    (year, month as u32, day as u32)
}

/// Compose a tournament_id-scoped `NewAttempt` for a synthetic / test row.
/// Not used in the live session path (which uses
/// [`outcome_to_new_attempt`] / [`failure_to_new_attempt`]); useful inside
/// tests that need to plant rows directly into the store. `pub` so
/// integration tests (external crates) can call it; `#[doc(hidden)]` to
/// keep it out of generated docs and the apparent public API.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn synthetic_attempt(
    tournament_id: TournamentId,
    game_config_id: &str,
    player1_id: &str,
    player2_id: &str,
    seed: u64,
    repetition_index: u32,
    attempt_index: u32,
    finished_at: &str,
    outcome: NewAttemptOutcome,
) -> NewAttempt {
    NewAttempt {
        key: AttemptKey {
            tournament_id,
            game_config_id: game_config_id.into(),
            player1_id: player1_id.into(),
            player2_id: player2_id.into(),
            seed,
            repetition_index,
            attempt_index,
        },
        finished_at: finished_at.into(),
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pyrat_host::match_host::MatchResult;
    use pyrat_host::player::PlayerIdentity;
    use pyrat_host::wire::{GameResult, Player};
    use pyrat_orchestrator::MatchId;

    use super::*;

    fn descriptor() -> EvalMatchDescriptor {
        EvalMatchDescriptor {
            match_id: MatchId(1),
            tournament_id: TournamentId(1),
            game_config_id: "gc".into(),
            player1_id: "a".into(),
            player2_id: "b".into(),
            seed: 7,
            repetition_index: 0,
            attempt_index: 0,
            planned_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn identity(slot: Player) -> PlayerIdentity {
        PlayerIdentity {
            name: "x".into(),
            author: "x".into(),
            agent_id: "x".into(),
            slot,
        }
    }

    #[test]
    fn classic_game_config_maps_to_record() {
        let cfg = GameConfig::classic(7, 5, 3);
        let rec = game_config_to_record(&cfg).unwrap();
        assert_eq!(rec.width, 7);
        assert_eq!(rec.height, 5);
        assert_eq!(rec.cheese_count, 3);
        assert!(rec.cheese_symmetric);
        assert!(rec.connected);
        assert!(rec.symmetric);
    }

    /// Pin the SQLite-format string for a few known instants. The values
    /// here come from cross-checking with `python -c "import datetime;
    /// print(datetime.datetime.utcfromtimestamp(N))"`.
    #[test]
    fn format_sqlite_datetime_matches_known_instants() {
        // 2024-02-29 (leap day) 00:00:00 UTC
        let t = UNIX_EPOCH + Duration::from_secs(1_709_164_800);
        assert_eq!(format_sqlite_datetime(t), "2024-02-29 00:00:00");
        // 2026-05-08 12:34:56 UTC
        let t = UNIX_EPOCH + Duration::from_secs(1_778_243_696);
        assert_eq!(format_sqlite_datetime(t), "2026-05-08 12:34:56");
        // 2000-03-01 00:00:00 UTC (post leap day, exercises Hinnant's mp branch)
        let t = UNIX_EPOCH + Duration::from_secs(951_868_800);
        assert_eq!(format_sqlite_datetime(t), "2000-03-01 00:00:00");
    }

    /// Epoch boundary: 1970-01-01 00:00:00.
    #[test]
    fn format_sqlite_datetime_handles_epoch() {
        assert_eq!(format_sqlite_datetime(UNIX_EPOCH), "1970-01-01 00:00:00");
    }

    #[test]
    fn outcome_maps_to_new_attempt_success() {
        let out = MatchOutcome {
            descriptor: descriptor(),
            started_at: UNIX_EPOCH + Duration::from_secs(100),
            finished_at: UNIX_EPOCH + Duration::from_secs(200),
            result: MatchResult {
                result: GameResult::Player1,
                player1_score: 5.0,
                player2_score: 3.0,
                turns_played: 50,
            },
            players: [identity(Player::Player1), identity(Player::Player2)],
        };
        let attempt = outcome_to_new_attempt(&out);
        assert_eq!(attempt.key.tournament_id, TournamentId(1));
        assert_eq!(attempt.key.attempt_index, 0);
        assert_eq!(attempt.finished_at, "1970-01-01 00:03:20");
        match attempt.outcome {
            NewAttemptOutcome::Success {
                player1_score,
                player2_score,
                turns,
                started_at,
            } => {
                assert!((player1_score - 5.0).abs() < 1e-9);
                assert!((player2_score - 3.0).abs() < 1e-9);
                assert_eq!(turns, 50);
                assert_eq!(started_at, "1970-01-01 00:01:40");
            },
            NewAttemptOutcome::Failure { .. } => panic!("expected Success outcome"),
        }
    }

    #[test]
    fn failure_maps_to_new_attempt_with_reason_string() {
        let fail = MatchFailure {
            descriptor: descriptor(),
            started_at: None,
            failed_at: UNIX_EPOCH + Duration::from_secs(50),
            reason: FailureReason::SpawnFailed,
            players: None,
            durable_record: true,
        };
        let attempt = failure_to_new_attempt(&fail);
        match attempt.outcome {
            NewAttemptOutcome::Failure {
                failure_reason,
                started_at,
            } => {
                assert_eq!(failure_reason, "spawn_failed");
                assert!(started_at.is_none());
            },
            NewAttemptOutcome::Success { .. } => panic!("expected Failure outcome"),
        }
    }

    #[test]
    fn failure_reason_string_includes_payload() {
        let s = failure_reason_string(&FailureReason::ProtocolError("timeout: setup".into()));
        assert_eq!(s, "protocol_error: timeout: setup");
    }
}
