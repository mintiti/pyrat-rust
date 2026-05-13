//! Planner trait + concrete implementations.
//!
//! A planner reads `TournamentState` (durable) and tracks its own pending
//! set (which `(matchup_key, attempt_index)` slots have been issued but not
//! yet observed). On each `next_batch`, it picks unfinished slots up to
//! `capacity` and emits ready-to-submit `Matchup<EvalMatchDescriptor>`s.
//!
//! Seeds are derived statelessly from the matchup key, so retries replay
//! the exact same seeded match.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use pyrat::game::builder::GameConfig;
use pyrat_eval_store::TournamentId;
use pyrat_orchestrator::{MatchId, Matchup, PlayerSpec, Timing};
use sha2::{Digest, Sha256};

use crate::descriptor::EvalMatchDescriptor;
use crate::observation::Observation;
use crate::state::{MatchupKey, MatchupOutcome, PlayerId, TournamentState};

/// Stateless seed for one matchup slot.
///
/// Same `MatchupKey` always yields the same seed, regardless of scheduling
/// order, parallelism, or resume. `attempt_index` is intentionally not in
/// the derivation: retries replay the exact same seeded match (deterministic
/// engine bugs surface every retry — that's correct, the planner gives up
/// after `max_failures_per_pair`; flaky environmental failures are the use
/// case where retries help).
///
/// The high bit is masked because SQLite `INTEGER` is signed `i64`; the
/// store's `record_attempt` rejects values above `i64::MAX` as a defense in
/// depth. 2^63 is still cosmically huge.
pub fn matchup_seed(
    tournament_seed: u64,
    p1: &str,
    p2: &str,
    game_config_id: &str,
    repetition_index: u32,
) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(tournament_seed.to_le_bytes());
    hasher.update(p1.as_bytes());
    hasher.update(p2.as_bytes());
    hasher.update(game_config_id.as_bytes());
    hasher.update(repetition_index.to_le_bytes());
    let bytes = hasher.finalize();
    let raw = u64::from_le_bytes(bytes[..8].try_into().expect("sha256 yields 32 bytes"));
    raw & (i64::MAX as u64)
}

/// One participant resolved from a `PlayerId` to a runnable `PlayerSpec`.
/// The session resolves these from `tournament_players` rows + the user's
/// `PlayerSpec` registry; planners never materialize new ones.
#[derive(Clone)]
pub struct ResolvedPlayer {
    pub id: PlayerId,
    pub spec: PlayerSpec,
}

impl std::fmt::Debug for ResolvedPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedPlayer")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

pub trait Planner: Send {
    /// Pick up to `capacity` new matchups from the current state. Returns
    /// empty when there's nothing left to issue right now (either the
    /// tournament is done or every unfinished slot already has a pending
    /// attempt). Combine with [`Planner::is_done`] to distinguish.
    ///
    /// `allocate_match_id` is a callback into the caller's id allocator
    /// (typically the orchestrator's). Keeping it as a closure means the
    /// planner doesn't depend on `MatchIdAllocator` directly.
    fn next_batch(
        &mut self,
        state: &TournamentState,
        capacity: usize,
        allocate_match_id: &mut dyn FnMut() -> MatchId,
    ) -> Vec<Matchup<EvalMatchDescriptor>>;

    /// Notification that one lifecycle event was observed. Used to clear
    /// the planner's pending set.
    fn on_observation(&mut self, observation: &Observation);

    /// True when no further matchups will ever be issued, regardless of
    /// capacity. Tournament terminates when this returns true and no
    /// matches are in flight.
    fn is_done(&self, state: &TournamentState) -> bool;

    /// Tournament this planner is configured to drive. `EvalSession::start`
    /// cross-checks this against the resumed `TournamentId` so a caller
    /// can't accidentally point a planner at the wrong tournament.
    fn tournament_id(&self) -> TournamentId;

    /// Player ids in slot order. Compared against the stored
    /// `tournament_players` rows on resume.
    fn expected_players(&self) -> Vec<&str>;

    /// Content-hash id of the game config this planner uses. Compared
    /// against the stored `tournaments.game_config_id` on resume.
    fn expected_game_config_id(&self) -> &str;

    /// Tournament-level seed driving `matchup_seed`. Compared against the
    /// stored `tournaments.tournament_seed` on resume. Two resumes with
    /// different seeds would replay different games and silently fragment
    /// the tournament.
    fn expected_tournament_seed(&self) -> u64;

    /// `target_games_per_matchup` if the planner has one, else `None`.
    /// Round-robin returns `Some(target_per_pair)`; gauntlet doesn't have
    /// a per-pair target so it returns `None`.
    fn expected_target_per_pair(&self) -> Option<u32>;
}

// ---------------------------------------------------------------------------
// Round-robin
// ---------------------------------------------------------------------------

/// All-vs-all tournament. Each unordered pair `{p1, p2}` plays
/// `target_per_pair` games. `repetition_index` distinguishes them; sides are
/// fixed by lexicographic player id (player1 = lex_min, player2 = lex_max).
/// Side alternation is a future enhancement (orthogonal to v1).
#[derive(Clone)]
pub struct RoundRobinPlannerConfig {
    pub players: Vec<ResolvedPlayer>,
    pub game_config: GameConfig,
    pub game_config_id: String,
    pub timing: Timing,
    pub tournament_id: TournamentId,
    pub target_per_pair: u32,
    pub max_failures_per_pair: u32,
    pub tournament_seed: u64,
}

pub struct RoundRobinPlanner {
    config: RoundRobinPlannerConfig,
    /// Computed once at construction. `n*(n-1)/2` entries, immutable for
    /// the life of the planner — no reason to rebuild per `next_batch`.
    pair_indices: Vec<(usize, usize)>,
    /// `(matchup_key, attempt_index)` of every matchup we've submitted but
    /// haven't seen a terminal observation for yet.
    pending: HashMap<MatchupKey, HashSet<u32>>,
}

impl RoundRobinPlanner {
    pub fn new(config: RoundRobinPlannerConfig) -> Self {
        let n = config.players.len();
        let mut pair_indices = Vec::with_capacity(n * (n.saturating_sub(1)) / 2);
        for i in 0..n {
            for j in (i + 1)..n {
                pair_indices.push((i, j));
            }
        }
        Self {
            config,
            pair_indices,
            pending: HashMap::new(),
        }
    }
}

impl Planner for RoundRobinPlanner {
    fn next_batch(
        &mut self,
        state: &TournamentState,
        capacity: usize,
        allocate_match_id: &mut dyn FnMut() -> MatchId,
    ) -> Vec<Matchup<EvalMatchDescriptor>> {
        if capacity == 0 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(capacity);
        let ctx = SlotContext::for_round_robin(&self.config);
        for &(i, j) in &self.pair_indices {
            for rep in 0..self.config.target_per_pair {
                if out.len() == capacity {
                    return out;
                }
                let a = &self.config.players[i];
                let b = &self.config.players[j];
                if let Some(matchup) =
                    build_slot(a, b, rep, &ctx, state, &mut self.pending, allocate_match_id)
                {
                    out.push(matchup);
                }
            }
        }
        out
    }

    fn on_observation(&mut self, observation: &Observation) {
        clear_pending(&mut self.pending, observation);
    }

    fn is_done(&self, state: &TournamentState) -> bool {
        self.pair_indices.iter().all(|&(i, j)| {
            let a = &self.config.players[i];
            let b = &self.config.players[j];
            (0..self.config.target_per_pair).all(|rep| {
                let key = MatchupKey::from_pair(&a.id, &b.id, &self.config.game_config_id, rep);
                slot_done(&key, state, self.config.max_failures_per_pair)
                    && !self
                        .pending
                        .get(&key)
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
            })
        })
    }

    fn tournament_id(&self) -> TournamentId {
        self.config.tournament_id
    }

    fn expected_players(&self) -> Vec<&str> {
        self.config.players.iter().map(|p| p.id.as_str()).collect()
    }

    fn expected_game_config_id(&self) -> &str {
        &self.config.game_config_id
    }

    fn expected_tournament_seed(&self) -> u64 {
        self.config.tournament_seed
    }

    fn expected_target_per_pair(&self) -> Option<u32> {
        Some(self.config.target_per_pair)
    }
}

// ---------------------------------------------------------------------------
// Gauntlet (challenger vs many opponents)
// ---------------------------------------------------------------------------

/// One challenger plays each opponent `target_each` games. Use case: a new
/// bot version vs the rest of the pool, when full round-robin is overkill.
#[derive(Clone)]
pub struct GauntletPlannerConfig {
    pub challenger: ResolvedPlayer,
    pub opponents: Vec<ResolvedPlayer>,
    pub game_config: GameConfig,
    pub game_config_id: String,
    pub timing: Timing,
    pub tournament_id: TournamentId,
    pub target_each: u32,
    pub max_failures_per_pair: u32,
    pub tournament_seed: u64,
}

pub struct GauntletPlanner {
    config: GauntletPlannerConfig,
    pending: HashMap<MatchupKey, HashSet<u32>>,
}

impl GauntletPlanner {
    pub fn new(config: GauntletPlannerConfig) -> Self {
        Self {
            config,
            pending: HashMap::new(),
        }
    }
}

impl Planner for GauntletPlanner {
    fn next_batch(
        &mut self,
        state: &TournamentState,
        capacity: usize,
        allocate_match_id: &mut dyn FnMut() -> MatchId,
    ) -> Vec<Matchup<EvalMatchDescriptor>> {
        if capacity == 0 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(capacity);
        let ctx = SlotContext::for_gauntlet(&self.config);
        for opp in &self.config.opponents {
            for rep in 0..self.config.target_each {
                if out.len() == capacity {
                    return out;
                }
                if let Some(matchup) = build_slot(
                    &self.config.challenger,
                    opp,
                    rep,
                    &ctx,
                    state,
                    &mut self.pending,
                    allocate_match_id,
                ) {
                    out.push(matchup);
                }
            }
        }
        out
    }

    fn on_observation(&mut self, observation: &Observation) {
        clear_pending(&mut self.pending, observation);
    }

    fn is_done(&self, state: &TournamentState) -> bool {
        self.config.opponents.iter().all(|opp| {
            (0..self.config.target_each).all(|rep| {
                let key = MatchupKey::from_pair(
                    &self.config.challenger.id,
                    &opp.id,
                    &self.config.game_config_id,
                    rep,
                );
                slot_done(&key, state, self.config.max_failures_per_pair)
                    && !self
                        .pending
                        .get(&key)
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
            })
        })
    }

    fn tournament_id(&self) -> TournamentId {
        self.config.tournament_id
    }

    fn expected_players(&self) -> Vec<&str> {
        // Slot 0 is the challenger; opponents follow in declaration order.
        std::iter::once(self.config.challenger.id.as_str())
            .chain(self.config.opponents.iter().map(|p| p.id.as_str()))
            .collect()
    }

    fn expected_game_config_id(&self) -> &str {
        &self.config.game_config_id
    }

    fn expected_tournament_seed(&self) -> u64 {
        self.config.tournament_seed
    }

    fn expected_target_per_pair(&self) -> Option<u32> {
        // Gauntlet's "target_each" is per opponent — same semantic shape
        // as round-robin's target_per_pair, so we surface it here for
        // consistency with the resume cross-check.
        Some(self.config.target_each)
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn slot_done(key: &MatchupKey, state: &TournamentState, max_failures: u32) -> bool {
    let history = state.history.get(key).map(Vec::as_slice).unwrap_or(&[]);
    let success = history
        .iter()
        .any(|a| matches!(a.outcome, MatchupOutcome::Success { .. }));
    if success {
        return true;
    }
    let failures = history
        .iter()
        .filter(|a| matches!(a.outcome, MatchupOutcome::Failure))
        .count();
    failures as u32 >= max_failures
}

fn clear_pending(pending: &mut HashMap<MatchupKey, HashSet<u32>>, observation: &Observation) {
    let desc = match observation {
        Observation::Finished { descriptor } | Observation::Failed { descriptor, .. } => descriptor,
        Observation::Queued { .. } | Observation::Started { .. } => return,
    };
    let key = MatchupKey::from_descriptor(desc);
    if let Some(set) = pending.get_mut(&key) {
        set.remove(&desc.attempt_index);
    }
}

/// Tournament-level invariants `build_slot` reads. Bundling these into one
/// struct keeps both planner impls calling `build_slot` with three clear
/// arguments (the pair, this context, the live cursors) instead of twelve
/// positional fields.
struct SlotContext<'a> {
    game_config: &'a GameConfig,
    game_config_id: &'a str,
    timing: Timing,
    tournament_id: TournamentId,
    tournament_seed: u64,
    max_failures: u32,
}

impl<'a> SlotContext<'a> {
    fn for_round_robin(config: &'a RoundRobinPlannerConfig) -> Self {
        Self {
            game_config: &config.game_config,
            game_config_id: &config.game_config_id,
            timing: config.timing,
            tournament_id: config.tournament_id,
            tournament_seed: config.tournament_seed,
            max_failures: config.max_failures_per_pair,
        }
    }

    fn for_gauntlet(config: &'a GauntletPlannerConfig) -> Self {
        Self {
            game_config: &config.game_config,
            game_config_id: &config.game_config_id,
            timing: config.timing,
            tournament_id: config.tournament_id,
            tournament_seed: config.tournament_seed,
            max_failures: config.max_failures_per_pair,
        }
    }
}

fn build_slot(
    a: &ResolvedPlayer,
    b: &ResolvedPlayer,
    repetition_index: u32,
    ctx: &SlotContext<'_>,
    state: &TournamentState,
    pending: &mut HashMap<MatchupKey, HashSet<u32>>,
    allocate_match_id: &mut dyn FnMut() -> MatchId,
) -> Option<Matchup<EvalMatchDescriptor>> {
    // Lex-sort players for slot 0/1 so the planner submits descriptors in
    // canonical orientation. `MatchupKey::from_pair` already canonicalizes
    // its own player_ids, but the descriptor's slot order is what the
    // engine sees.
    let (p1, p2) = if a.id <= b.id { (a, b) } else { (b, a) };
    let key = MatchupKey::from_pair(&p1.id, &p2.id, ctx.game_config_id, repetition_index);

    if slot_done(&key, state, ctx.max_failures) {
        return None;
    }
    let pending_set = pending.entry(key.clone()).or_default();
    if !pending_set.is_empty() {
        // Already issued one for this slot; wait for terminal before retrying.
        return None;
    }

    let history_len = state.history.get(&key).map(Vec::len).unwrap_or(0);
    let attempt_index = history_len as u32;
    pending_set.insert(attempt_index);

    let seed = matchup_seed(
        ctx.tournament_seed,
        &key.player1_id,
        &key.player2_id,
        ctx.game_config_id,
        repetition_index,
    );
    let descriptor = EvalMatchDescriptor {
        match_id: allocate_match_id(),
        tournament_id: ctx.tournament_id,
        game_config_id: ctx.game_config_id.to_owned(),
        player1_id: key.player1_id.clone(),
        player2_id: key.player2_id.clone(),
        seed,
        repetition_index,
        attempt_index,
        planned_at: SystemTime::now(),
    };

    Some(Matchup {
        descriptor,
        game_config: ctx.game_config.clone(),
        players: [p1.spec.clone(), p2.spec.clone()],
        timing: ctx.timing,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::SystemTime;

    use pyrat::Direction;
    use pyrat_bot_api::Options;
    use pyrat_host::player::{EmbeddedBot, EmbeddedCtx};
    use pyrat_host::wire::TimingMode;
    use pyrat_orchestrator::{
        EmbeddedBotFactory, FailureReason, MatchFailure, MatchIdAllocator, MatchOutcome, PlayerSpec,
    };
    use pyrat_protocol::HashedTurnState;

    use super::*;

    struct StubBot;
    impl Options for StubBot {}
    impl EmbeddedBot for StubBot {
        fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
            Direction::Stay
        }
    }

    fn embedded(id: &str) -> ResolvedPlayer {
        let factory: EmbeddedBotFactory = Arc::new(|| Box::new(StubBot));
        ResolvedPlayer {
            id: id.into(),
            spec: PlayerSpec::Embedded {
                agent_id: id.into(),
                name: id.into(),
                author: "x".into(),
                factory,
            },
        }
    }

    fn timing() -> Timing {
        Timing {
            mode: TimingMode::Wait,
            move_timeout_ms: 1000,
            preprocessing_timeout_ms: 5000,
        }
    }

    fn tiny_round_robin(target: u32, max_failures: u32) -> RoundRobinPlanner {
        RoundRobinPlanner::new(RoundRobinPlannerConfig {
            players: vec![embedded("a"), embedded("b"), embedded("c")],
            game_config: GameConfig::classic(7, 5, 3),
            game_config_id: "gc".into(),
            timing: timing(),
            tournament_id: TournamentId(1),
            target_per_pair: target,
            max_failures_per_pair: max_failures,
            tournament_seed: 0xC0FFEE,
        })
    }

    fn finished_obs(d: EvalMatchDescriptor) -> Observation {
        Observation::Finished { descriptor: d }
    }

    fn failed_obs(d: EvalMatchDescriptor, durable: bool) -> Observation {
        Observation::Failed {
            descriptor: d,
            durable_record: durable,
            reason: FailureReason::SpawnFailed,
        }
    }

    fn fold_outcome(
        state: &mut TournamentState,
        d: EvalMatchDescriptor,
        p1_score: f64,
        p2_score: f64,
    ) {
        use pyrat_host::match_host::MatchResult;
        use pyrat_host::player::PlayerIdentity;
        use pyrat_host::wire::{GameResult, Player};
        let identity = |slot| PlayerIdentity {
            name: "x".into(),
            author: "x".into(),
            agent_id: "x".into(),
            slot,
        };
        state.apply(&pyrat_orchestrator::DriverEvent::MatchFinished {
            outcome: MatchOutcome {
                descriptor: d,
                started_at: SystemTime::UNIX_EPOCH,
                finished_at: SystemTime::UNIX_EPOCH,
                result: MatchResult {
                    result: GameResult::Draw,
                    player1_score: p1_score as f32,
                    player2_score: p2_score as f32,
                    turns_played: 50,
                },
                players: [identity(Player::Player1), identity(Player::Player2)],
            },
        });
    }

    fn fold_failure(state: &mut TournamentState, d: EvalMatchDescriptor, durable: bool) {
        state.apply(&pyrat_orchestrator::DriverEvent::MatchFailed {
            failure: MatchFailure {
                descriptor: d,
                started_at: None,
                failed_at: SystemTime::UNIX_EPOCH,
                reason: FailureReason::SpawnFailed,
                players: None,
                durable_record: durable,
            },
        });
    }

    /// 3 players × 1 game per pair = 3 unique matchups.
    /// All MatchupKeys are distinct: planner issues every slot exactly once.
    #[test]
    fn round_robin_target_one_yields_three_pair_matchups() {
        let mut p = tiny_round_robin(1, 3);
        let alloc = MatchIdAllocator::new();
        let state = TournamentState::empty(TournamentId(1));
        let batch = p.next_batch(&state, 100, &mut || alloc.allocate());
        assert_eq!(batch.len(), 3);
        // Distinct (player1, player2) lex-canonical pairs.
        let mut pairs: Vec<_> = batch
            .iter()
            .map(|m| {
                (
                    m.descriptor.player1_id.clone(),
                    m.descriptor.player2_id.clone(),
                )
            })
            .collect();
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("a".into(), "b".into()),
                ("a".into(), "c".into()),
                ("b".into(), "c".into()),
            ]
        );
    }

    /// Capacity caps the batch: 3 pairs × 2 games = 6 slots, capacity=2.
    /// Only 2 matchups returned; remaining slots issued on later calls.
    #[test]
    fn next_batch_respects_capacity() {
        let mut p = tiny_round_robin(2, 3);
        let alloc = MatchIdAllocator::new();
        let state = TournamentState::empty(TournamentId(1));
        let batch = p.next_batch(&state, 2, &mut || alloc.allocate());
        assert_eq!(batch.len(), 2);
    }

    /// Pending set prevents double-issuing: after issuing slot (a,b,rep=0),
    /// the next call (with state still showing zero history for that slot)
    /// must skip it until on_observation clears the pending entry.
    #[test]
    fn pending_set_prevents_double_issue() {
        let mut p = tiny_round_robin(1, 3);
        let alloc = MatchIdAllocator::new();
        let mut state = TournamentState::empty(TournamentId(1));
        let batch1 = p.next_batch(&state, 100, &mut || alloc.allocate());
        assert_eq!(batch1.len(), 3);
        // Without observation, planner sees nothing it can issue.
        let batch2 = p.next_batch(&state, 100, &mut || alloc.allocate());
        assert!(batch2.is_empty());
        // After we observe a finished + state.apply, that slot is done; the
        // other two are still pending.
        let m = batch1[0].clone();
        fold_outcome(&mut state, m.descriptor.clone(), 5.0, 3.0);
        p.on_observation(&finished_obs(m.descriptor));
        let batch3 = p.next_batch(&state, 100, &mut || alloc.allocate());
        assert_eq!(batch3.len(), 0); // others still in flight
    }

    /// Failure observation lets the planner re-issue at attempt_index+1
    /// (until max_failures_per_pair is hit).
    #[test]
    fn failure_triggers_retry_at_next_attempt_index() {
        let mut p = tiny_round_robin(1, 3);
        let alloc = MatchIdAllocator::new();
        let mut state = TournamentState::empty(TournamentId(1));
        let batch1 = p.next_batch(&state, 100, &mut || alloc.allocate());
        let first = batch1[0].clone();
        assert_eq!(first.descriptor.attempt_index, 0);
        // Durable failure: history gets a Failure entry.
        fold_failure(&mut state, first.descriptor.clone(), true);
        p.on_observation(&failed_obs(first.descriptor.clone(), true));
        let batch2 = p.next_batch(&state, 100, &mut || alloc.allocate());
        // The retried slot should appear with attempt_index=1.
        let retry = batch2
            .iter()
            .find(|m| {
                m.descriptor.player1_id == first.descriptor.player1_id
                    && m.descriptor.player2_id == first.descriptor.player2_id
            })
            .expect("retry of the failed pair");
        assert_eq!(retry.descriptor.attempt_index, 1);
    }

    /// `durable_record == false` means no durable row exists; the planner
    /// must re-issue at the *same* attempt_index (silent retry).
    #[test]
    fn non_durable_failure_retries_at_same_attempt_index() {
        let mut p = tiny_round_robin(1, 3);
        let alloc = MatchIdAllocator::new();
        let mut state = TournamentState::empty(TournamentId(1));
        let batch1 = p.next_batch(&state, 100, &mut || alloc.allocate());
        let first = batch1[0].clone();
        assert_eq!(first.descriptor.attempt_index, 0);
        // Non-durable failure: history is NOT touched (state.apply enforces
        // this).
        fold_failure(&mut state, first.descriptor.clone(), false);
        p.on_observation(&failed_obs(first.descriptor.clone(), false));
        let batch2 = p.next_batch(&state, 100, &mut || alloc.allocate());
        let retry = batch2
            .iter()
            .find(|m| {
                m.descriptor.player1_id == first.descriptor.player1_id
                    && m.descriptor.player2_id == first.descriptor.player2_id
            })
            .expect("silent retry of the lost pair");
        assert_eq!(retry.descriptor.attempt_index, 0);
        // And the seed is identical: same matchup key → same seed.
        assert_eq!(retry.descriptor.seed, first.descriptor.seed);
    }

    /// After max_failures_per_pair durable failures, the planner gives up
    /// on that slot and is_done returns true (assuming all other slots also
    /// resolved).
    #[test]
    fn max_failures_per_pair_terminates_slot() {
        // Single pair to keep the assertion clean.
        let mut p = RoundRobinPlanner::new(RoundRobinPlannerConfig {
            players: vec![embedded("a"), embedded("b")],
            game_config: GameConfig::classic(7, 5, 3),
            game_config_id: "gc".into(),
            timing: timing(),
            tournament_id: TournamentId(1),
            target_per_pair: 1,
            max_failures_per_pair: 2,
            tournament_seed: 1,
        });
        let alloc = MatchIdAllocator::new();
        let mut state = TournamentState::empty(TournamentId(1));
        for _ in 0..2 {
            let batch = p.next_batch(&state, 100, &mut || alloc.allocate());
            assert_eq!(batch.len(), 1);
            let m = batch[0].clone();
            fold_failure(&mut state, m.descriptor.clone(), true);
            p.on_observation(&failed_obs(m.descriptor, true));
        }
        let batch3 = p.next_batch(&state, 100, &mut || alloc.allocate());
        assert!(batch3.is_empty());
        assert!(p.is_done(&state));
    }

    /// Same matchup key always yields the same seed, regardless of caller
    /// order. Pins the stateless-derivation contract.
    #[test]
    fn matchup_seed_is_pure() {
        let s1 = matchup_seed(0xDEAD, "a", "b", "gc", 0);
        let s2 = matchup_seed(0xDEAD, "a", "b", "gc", 0);
        assert_eq!(s1, s2);
        let s3 = matchup_seed(0xDEAD, "a", "b", "gc", 1);
        assert_ne!(s1, s3);
        // High bit always masked.
        assert!(s1 <= i64::MAX as u64);
    }
}
