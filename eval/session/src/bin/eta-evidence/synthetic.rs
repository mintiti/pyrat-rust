use std::fs;
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};
use pyrat_eval::orchestrator::{
    AttemptDetail, AttemptTerminal, ExecutionPhase, ExecutionTimingEvent, MatchId, MonotonicTime,
    TimingFailureReason, TimingGameResult, TimingPlayer, TimingTimeoutPhase,
};
use pyrat_eval::{
    DispatchPolicy, EvalMatchDescriptor, PlannedSlot, SeatOrientation, TournamentTimingTrace,
    TIMING_TRACE_SCHEMA_VERSION,
};
use pyrat_eval_store::TournamentId;

pub(crate) const FINAL_PARTIAL_BATCH_FILE: &str = "synthetic-final-partial-batch.json";
pub(crate) const ACTIVE_STRAGGLER_FILE: &str = "synthetic-active-straggler.json";
pub(crate) const DURABLE_RETRY_FILE: &str = "synthetic-durable-retry.json";
pub(crate) const MIXED_PAIR_MAJOR_C1_FILE: &str = "synthetic-mixed-pair-major-c1.json";
pub(crate) const MIXED_BALANCED_C1_FILE: &str = "synthetic-mixed-bot-balanced-c1.json";
pub(crate) const MIXED_PAIR_MAJOR_C2_FILE: &str = "synthetic-mixed-pair-major-c2.json";
pub(crate) const MIXED_BALANCED_C2_FILE: &str = "synthetic-mixed-bot-balanced-c2.json";
pub(crate) const MIXED_PAIR_MAJOR_C4_FILE: &str = "synthetic-mixed-pair-major-c4.json";
pub(crate) const MIXED_BALANCED_C4_FILE: &str = "synthetic-mixed-bot-balanced-c4.json";

pub(crate) fn write_cases(output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("create synthetic corpus directory {}", output_dir.display()))?;
    for (name, trace) in cases() {
        let path = output_dir.join(name);
        let encoded = serde_json::to_vec_pretty(&trace).context("encode synthetic timing trace")?;
        fs::write(&path, encoded)
            .with_context(|| format!("write synthetic timing trace {}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn cases() -> Vec<(&'static str, TournamentTimingTrace)> {
    let mut cases = vec![
        (FINAL_PARTIAL_BATCH_FILE, final_partial_batch()),
        (ACTIVE_STRAGGLER_FILE, active_straggler()),
        (DURABLE_RETRY_FILE, durable_retry()),
    ];
    cases.extend([
        (
            MIXED_PAIR_MAJOR_C1_FILE,
            mixed_dispatch(DispatchPolicy::PairMajorV1, 1),
        ),
        (
            MIXED_BALANCED_C1_FILE,
            mixed_dispatch(DispatchPolicy::BotBalancedCircleV1, 1),
        ),
        (
            MIXED_PAIR_MAJOR_C2_FILE,
            mixed_dispatch(DispatchPolicy::PairMajorV1, 2),
        ),
        (
            MIXED_BALANCED_C2_FILE,
            mixed_dispatch(DispatchPolicy::BotBalancedCircleV1, 2),
        ),
        (
            MIXED_PAIR_MAJOR_C4_FILE,
            mixed_dispatch(DispatchPolicy::PairMajorV1, 4),
        ),
        (
            MIXED_BALANCED_C4_FILE,
            mixed_dispatch(DispatchPolicy::BotBalancedCircleV1, 4),
        ),
    ]);
    cases
}

fn final_partial_batch() -> TournamentTimingTrace {
    let d0 = descriptor(0, 0, 0);
    let d1 = descriptor(1, 1, 0);
    let d2 = descriptor(2, 2, 0);
    let mut trace = TraceBuilder::new(2, 1, 3);
    trace.queued(0, &d0, 0);
    trace.queued(0, &d1, 0);
    trace.push(0, &d0, 1, ExecutionPhase::PermitAcquired);
    trace.push(0, &d1, 2, ExecutionPhase::PermitAcquired);
    trace.push(1_000_000, &d0, 2, ExecutionPhase::PlayingStarted);
    trace.push(1_000_000, &d1, 2, ExecutionPhase::PlayingStarted);
    trace.push(
        2_000_000,
        &d0,
        2,
        ExecutionPhase::ActionCollectionStarted { turn: 0 },
    );
    trace.push(
        2_000_000,
        &d1,
        2,
        ExecutionPhase::ActionCollectionStarted { turn: 0 },
    );
    trace.push(3_000_000, &d0, 2, ExecutionPhase::TurnCompleted { turn: 0 });
    trace.push(3_000_000, &d1, 2, ExecutionPhase::TurnCompleted { turn: 0 });
    trace.push(
        4_000_000,
        &d1,
        2,
        ExecutionPhase::ActionCollectionStarted { turn: 1 },
    );
    trace.finished(10_000_000, &d0, 2);
    trace.finished(10_000_000, &d1, 2);
    trace.push(10_000_000, &d0, 1, ExecutionPhase::PermitReleased);
    trace.push(10_000_000, &d1, 0, ExecutionPhase::PermitReleased);
    trace.queued(10_000_000, &d2, 0);
    trace.push(10_000_000, &d2, 1, ExecutionPhase::PermitAcquired);
    trace.finished(20_000_000, &d2, 1);
    trace.push(20_000_000, &d2, 0, ExecutionPhase::PermitReleased);
    trace.finish()
}

fn active_straggler() -> TournamentTimingTrace {
    let d0 = descriptor(0, 0, 0);
    let d1 = descriptor(1, 1, 0);
    let d2 = descriptor(2, 2, 0);
    let d3 = descriptor(3, 3, 0);
    let mut trace = TraceBuilder::new(2, 1, 4);
    trace.queued(0, &d0, 0);
    trace.queued(0, &d1, 0);
    trace.push(0, &d0, 1, ExecutionPhase::PermitAcquired);
    trace.push(0, &d1, 2, ExecutionPhase::PermitAcquired);
    trace.finished(10_000_000, &d0, 2);
    trace.push(10_000_000, &d0, 1, ExecutionPhase::PermitReleased);
    trace.queued(10_000_000, &d2, 1);
    trace.push(10_000_000, &d2, 2, ExecutionPhase::PermitAcquired);
    trace.push(11_000_000, &d2, 2, ExecutionPhase::PlayingStarted);
    trace.push(
        12_000_000,
        &d2,
        2,
        ExecutionPhase::ActionCollectionStarted { turn: 0 },
    );
    trace.push(
        13_000_000,
        &d2,
        2,
        ExecutionPhase::TurnCompleted { turn: 0 },
    );
    trace.push(
        14_000_000,
        &d2,
        2,
        ExecutionPhase::ActionCollectionStarted { turn: 1 },
    );
    trace.finished(20_000_000, &d1, 2);
    trace.push(20_000_000, &d1, 1, ExecutionPhase::PermitReleased);
    trace.queued(20_000_000, &d3, 1);
    trace.push(20_000_000, &d3, 2, ExecutionPhase::PermitAcquired);
    trace.finished(21_000_000, &d3, 2);
    trace.push(21_000_000, &d3, 1, ExecutionPhase::PermitReleased);
    trace.finished(40_000_000, &d2, 1);
    trace.push(40_000_000, &d2, 0, ExecutionPhase::PermitReleased);
    trace.finish()
}

fn durable_retry() -> TournamentTimingTrace {
    let first = descriptor(0, 0, 0);
    let retry = descriptor(1, 0, 1);
    let mut trace = TraceBuilder::new(1, 2, 1);
    trace.queued(0, &first, 0);
    trace.push(0, &first, 1, ExecutionPhase::PermitAcquired);
    trace.failed(
        5_000_000,
        &first,
        1,
        TimingFailureReason::Timeout {
            player: TimingPlayer::Player1,
            phase: TimingTimeoutPhase::Move,
        },
    );
    trace.push(5_000_000, &first, 0, ExecutionPhase::PermitReleased);
    trace.queued(5_000_000, &retry, 0);
    trace.push(5_000_000, &retry, 1, ExecutionPhase::PermitAcquired);
    trace.finished(10_000_000, &retry, 1);
    trace.push(10_000_000, &retry, 0, ExecutionPhase::PermitReleased);
    trace.finish()
}

fn mixed_dispatch(policy: DispatchPolicy, max_parallel: usize) -> TournamentTimingTrace {
    let slots = mixed_slots();
    let issue_slots = mixed_issue_slots(policy, &slots);
    let mut trace = TraceBuilder::with_slots(policy, max_parallel, 2, slots);
    let mut pending = issue_slots.into_iter();
    let mut active = Vec::<SimAttempt>::new();
    let mut next_slot = pending.next();
    let mut now = 0u64;
    let mut next_match_id = 10_000u64;

    while next_slot.is_some() || !active.is_empty() {
        while active.len() < max_parallel {
            let Some(slot) = next_slot.take() else {
                break;
            };
            let descriptor = mixed_descriptor(next_match_id, &slot);
            next_match_id += 1;
            let players = seated_players(&slot);
            trace.push(
                now,
                &descriptor,
                active.len(),
                ExecutionPhase::Queued {
                    players: players.clone(),
                },
            );
            let acquired_concurrency = active.len() + 1;
            trace.push(
                now,
                &descriptor,
                acquired_concurrency,
                ExecutionPhase::PermitAcquired,
            );
            let overlaps = active
                .iter()
                .filter(|attempt| shares_player(&players, &attempt.players))
                .count() as u64;
            let base = mixed_duration_micros(&slot);
            let duration = base
                .saturating_mul(2 + overlaps)
                .checked_div(2)
                .unwrap_or(base);
            trace.push(
                now + duration / 5,
                &descriptor,
                acquired_concurrency,
                ExecutionPhase::PlayingStarted,
            );
            trace.push(
                now + duration / 2,
                &descriptor,
                acquired_concurrency,
                ExecutionPhase::TurnCompleted { turn: 4 },
            );
            active.push(SimAttempt {
                descriptor,
                slot,
                players,
                finished_at: now + duration,
            });
            next_slot = pending.next();
        }

        let Some(finished_at) = active.iter().map(|attempt| attempt.finished_at).min() else {
            break;
        };
        now = finished_at;
        let mut finished_ids: Vec<_> = active
            .iter()
            .filter(|attempt| attempt.finished_at == now)
            .map(|attempt| attempt.descriptor.match_id)
            .collect();
        finished_ids.sort();
        for match_id in finished_ids {
            let index = active
                .iter()
                .position(|attempt| attempt.descriptor.match_id == match_id)
                .expect("finished synthetic attempt remains active");
            let attempt = active.remove(index);
            let concurrency = active.len() + 1;
            trace.push(
                now,
                &attempt.descriptor,
                concurrency,
                ExecutionPhase::TerminalProduced {
                    outcome: AttemptTerminal::Finished,
                },
            );
            trace.push(
                now,
                &attempt.descriptor,
                concurrency,
                ExecutionPhase::TerminalDetail {
                    detail: mixed_outcome(&attempt.slot),
                },
            );
            trace.push(
                now,
                &attempt.descriptor,
                concurrency,
                ExecutionPhase::TerminalPersisted {
                    outcome: AttemptTerminal::Finished,
                },
            );
            trace.push(
                now,
                &attempt.descriptor,
                active.len(),
                ExecutionPhase::PermitReleased,
            );
        }
    }

    trace.finish()
}

struct SimAttempt {
    descriptor: EvalMatchDescriptor,
    slot: PlannedSlot,
    players: [String; 2],
    finished_at: u64,
}

fn mixed_slots() -> Vec<PlannedSlot> {
    let players = ["alpha", "beta", "gamma", "delta"];
    let mut slots = Vec::new();
    for first in 0..players.len() {
        for second in (first + 1)..players.len() {
            let (player1_id, player2_id) = if players[first] <= players[second] {
                (players[first], players[second])
            } else {
                (players[second], players[first])
            };
            for repetition_index in 0..2 {
                slots.push(PlannedSlot {
                    player1_id: player1_id.into(),
                    player2_id: player2_id.into(),
                    game_config_id: "synthetic-mixed".into(),
                    repetition_index,
                    orientation: if repetition_index == 0 {
                        SeatOrientation::Canonical
                    } else {
                        SeatOrientation::Flipped
                    },
                });
            }
        }
    }
    slots
}

fn mixed_issue_slots(policy: DispatchPolicy, slots: &[PlannedSlot]) -> Vec<PlannedSlot> {
    match policy {
        DispatchPolicy::PairMajorV1 => slots.to_vec(),
        DispatchPolicy::BotBalancedCircleV1 => {
            let rounds = [
                [("alpha", "delta"), ("beta", "gamma")],
                [("alpha", "gamma"), ("beta", "delta")],
                [("alpha", "beta"), ("delta", "gamma")],
            ];
            let mut ordered = Vec::with_capacity(slots.len());
            for repetition_index in 0..2 {
                for round in rounds {
                    for (first, second) in round {
                        ordered.push(
                            slots
                                .iter()
                                .find(|slot| {
                                    slot.player1_id == first
                                        && slot.player2_id == second
                                        && slot.repetition_index == repetition_index
                                })
                                .expect("circle pair exists in synthetic schedule")
                                .clone(),
                        );
                    }
                }
            }
            ordered
        },
    }
}

fn mixed_descriptor(match_id: u64, slot: &PlannedSlot) -> EvalMatchDescriptor {
    let seed = slot
        .player1_id
        .bytes()
        .chain(slot.player2_id.bytes())
        .fold(u64::from(slot.repetition_index), |seed, byte| {
            seed.wrapping_mul(37).wrapping_add(u64::from(byte))
        });
    EvalMatchDescriptor {
        match_id: MatchId(match_id),
        tournament_id: TournamentId(2),
        game_config_id: slot.game_config_id.clone(),
        player1_id: slot.player1_id.clone(),
        player2_id: slot.player2_id.clone(),
        seed,
        repetition_index: slot.repetition_index,
        attempt_index: 0,
        orientation: slot.orientation,
        planned_at: SystemTime::UNIX_EPOCH,
    }
}

fn seated_players(slot: &PlannedSlot) -> [String; 2] {
    match slot.orientation {
        SeatOrientation::Canonical => [slot.player1_id.clone(), slot.player2_id.clone()],
        SeatOrientation::Flipped => [slot.player2_id.clone(), slot.player1_id.clone()],
    }
}

fn shares_player(left: &[String; 2], right: &[String; 2]) -> bool {
    left.iter().any(|player| right.contains(player))
}

fn mixed_duration_micros(slot: &PlannedSlot) -> u64 {
    let pair = (slot.player1_id.as_str(), slot.player2_id.as_str());
    let seconds = match pair {
        ("alpha", "beta") => 12,
        ("alpha", "gamma") => 4,
        ("alpha", "delta") => 8,
        ("beta", "gamma") => 5,
        ("beta", "delta") => 10,
        ("delta", "gamma") => 3,
        _ => 6,
    } + u64::from(slot.repetition_index);
    seconds * 1_000_000
}

fn mixed_outcome(slot: &PlannedSlot) -> AttemptDetail {
    let canonical_player1_wins = (slot.player1_id.bytes().map(u32::from).sum::<u32>()
        + slot.player2_id.bytes().map(u32::from).sum::<u32>()
        + slot.repetition_index)
        .is_multiple_of(2);
    let (result, player1_half_points, player2_half_points) =
        match (canonical_player1_wins, slot.orientation) {
            (true, SeatOrientation::Canonical) | (false, SeatOrientation::Flipped) => {
                (TimingGameResult::Player1, 8, 4)
            },
            (false, SeatOrientation::Canonical) | (true, SeatOrientation::Flipped) => {
                (TimingGameResult::Player2, 4, 8)
            },
        };
    AttemptDetail::Finished {
        result,
        player1_half_points,
        player2_half_points,
        turns_played: 20 + slot.repetition_index as u16,
    }
}

struct TraceBuilder {
    dispatch_policy: DispatchPolicy,
    max_parallel: usize,
    max_failures_per_slot: u32,
    slots: Vec<PlannedSlot>,
    events: Vec<ExecutionTimingEvent<EvalMatchDescriptor>>,
}

impl TraceBuilder {
    fn new(max_parallel: usize, max_failures_per_slot: u32, slot_count: u32) -> Self {
        Self {
            dispatch_policy: DispatchPolicy::PairMajorV1,
            max_parallel,
            max_failures_per_slot,
            slots: (0..slot_count).map(slot).collect(),
            events: Vec::new(),
        }
    }

    fn with_slots(
        dispatch_policy: DispatchPolicy,
        max_parallel: usize,
        max_failures_per_slot: u32,
        slots: Vec<PlannedSlot>,
    ) -> Self {
        Self {
            dispatch_policy,
            max_parallel,
            max_failures_per_slot,
            slots,
            events: Vec::new(),
        }
    }

    fn queued(&mut self, micros: u64, descriptor: &EvalMatchDescriptor, active_concurrency: usize) {
        self.push(
            micros,
            descriptor,
            active_concurrency,
            ExecutionPhase::Queued {
                players: ["a".into(), "b".into()],
            },
        );
    }

    fn finished(
        &mut self,
        micros: u64,
        descriptor: &EvalMatchDescriptor,
        active_concurrency: usize,
    ) {
        self.push(
            micros,
            descriptor,
            active_concurrency,
            ExecutionPhase::TerminalProduced {
                outcome: AttemptTerminal::Finished,
            },
        );
        self.push(
            micros,
            descriptor,
            active_concurrency,
            ExecutionPhase::TerminalDetail {
                detail: AttemptDetail::Finished {
                    result: TimingGameResult::Draw,
                    player1_half_points: 2,
                    player2_half_points: 2,
                    turns_played: 20,
                },
            },
        );
        self.push(
            micros,
            descriptor,
            active_concurrency,
            ExecutionPhase::TerminalPersisted {
                outcome: AttemptTerminal::Finished,
            },
        );
    }

    fn failed(
        &mut self,
        micros: u64,
        descriptor: &EvalMatchDescriptor,
        active_concurrency: usize,
        reason: TimingFailureReason,
    ) {
        let outcome = AttemptTerminal::Failed {
            durable_record: true,
        };
        self.push(
            micros,
            descriptor,
            active_concurrency,
            ExecutionPhase::TerminalProduced { outcome },
        );
        self.push(
            micros,
            descriptor,
            active_concurrency,
            ExecutionPhase::TerminalDetail {
                detail: AttemptDetail::Failed { reason },
            },
        );
        self.push(
            micros,
            descriptor,
            active_concurrency,
            ExecutionPhase::TerminalPersisted { outcome },
        );
    }

    fn push(
        &mut self,
        micros: u64,
        descriptor: &EvalMatchDescriptor,
        active_concurrency: usize,
        phase: ExecutionPhase,
    ) {
        self.events.push(ExecutionTimingEvent {
            sequence: self.events.len() as u64,
            observed_at: MonotonicTime::from_micros(micros),
            active_concurrency,
            descriptor: descriptor.clone(),
            phase,
        });
    }

    fn finish(self) -> TournamentTimingTrace {
        TournamentTimingTrace {
            schema_version: TIMING_TRACE_SCHEMA_VERSION,
            dispatch_policy: self.dispatch_policy,
            max_parallel: self.max_parallel,
            max_failures_per_slot: self.max_failures_per_slot,
            slots: self.slots,
            events: self.events,
        }
    }
}

fn slot(repetition_index: u32) -> PlannedSlot {
    PlannedSlot {
        player1_id: "a".into(),
        player2_id: "b".into(),
        game_config_id: "synthetic".into(),
        repetition_index,
        orientation: SeatOrientation::Canonical,
    }
}

fn descriptor(match_id: u64, repetition_index: u32, attempt_index: u32) -> EvalMatchDescriptor {
    EvalMatchDescriptor {
        match_id: MatchId(match_id),
        tournament_id: TournamentId(1),
        game_config_id: "synthetic".into(),
        player1_id: "a".into(),
        player2_id: "b".into(),
        seed: u64::from(repetition_index),
        repetition_index,
        attempt_index,
        orientation: SeatOrientation::Canonical,
        planned_at: SystemTime::UNIX_EPOCH,
    }
}

#[cfg(test)]
mod tests {
    use pyrat_eval::{compare_eta_models_walk_forward, EtaState};

    use super::*;

    #[test]
    fn every_synthetic_case_is_complete_and_walk_forward_replayable() {
        for (name, trace) in cases() {
            let comparison = compare_eta_models_walk_forward(&trace, 0)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(
                comparison.last().map(|point| point.m1p.estimate.state),
                Some(EtaState::Complete),
                "{name}"
            );
        }
    }
}
