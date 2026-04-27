//! Protocol types extracted from FlatBuffers messages.
//!
//! These types are the canonical representations of protocol data, shared by
//! the host and SDK. The [`crate::codec`] module converts between FlatBuffers
//! wire format and these types at the boundary.
//!
//! All position and direction fields use engine types (`Coordinates`, `Direction`).
//! The codec is the only place that touches wire representations.

use std::hash::{Hash, Hasher};
use std::ops::Deref;

use pyrat::{Coordinates, Direction};
use pyrat_wire::{GameResult, OptionType, Player, TimingMode};

// ── Direction conversion ────────────────────────────

/// Convert a wire Direction to an engine Direction.
pub fn wire_to_engine_direction(d: pyrat_wire::Direction) -> Direction {
    match d {
        pyrat_wire::Direction::Up => Direction::Up,
        pyrat_wire::Direction::Right => Direction::Right,
        pyrat_wire::Direction::Down => Direction::Down,
        pyrat_wire::Direction::Left => Direction::Left,
        pyrat_wire::Direction::Stay => Direction::Stay,
        _ => {
            debug_assert!(false, "unknown wire Direction discriminant: {}", d.0);
            Direction::Stay
        },
    }
}

/// Convert an engine Direction to a wire Direction.
///
/// Relies on engine `Direction` discriminants (0–4) matching wire `Direction`
/// constants. The `direction_conversion_roundtrip` test checks the 5 current
/// variants; if you add a new engine direction, update both the wire schema
/// and that test.
pub fn engine_to_wire_direction(d: Direction) -> pyrat_wire::Direction {
    pyrat_wire::Direction(d as u8)
}

// ── Bot option declaration ──────────────────────────

/// A bot-declared configurable option (from Identify).
///
/// Bots declare these in their Identify message to advertise knobs the host
/// or GUI can set before the match starts. Mirrors UCI option declarations.
#[derive(Debug, Clone)]
pub struct OptionDef {
    pub name: String,
    pub option_type: OptionType,
    pub default_value: String,
    pub min: i32,
    pub max: i32,
    pub choices: Vec<String>,
}

// ── Bot analysis info ───────────────────────────────

/// Analysis/debug info sent by a bot during thinking or preprocessing.
///
/// Tagged with player, turn, and state_hash for correlation. The host
/// forwards these to the event stream without inspecting them.
#[derive(Debug, Clone)]
pub struct Info {
    pub player: Player,
    pub multipv: u16,
    pub target: Option<Coordinates>,
    pub depth: u16,
    pub nodes: u32,
    pub score: Option<f32>,
    pub pv: Vec<Direction>,
    pub message: String,
    pub turn: u16,
    pub state_hash: u64,
}

// ── Match configuration ─────────────────────────────

/// A single mud passage between two cells, with the number of turns it takes
/// to cross.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MudEntry {
    pub pos1: Coordinates,
    pub pos2: Coordinates,
    pub turns: u8,
}

/// Match configuration sent to bots during the Lobby phase.
///
/// Contains the maze layout, player positions, cheese, timing, and
/// which players this connection controls.
#[derive(Debug, Clone)]
pub struct MatchConfig {
    pub width: u8,
    pub height: u8,
    pub max_turns: u16,
    pub walls: Vec<(Coordinates, Coordinates)>,
    pub mud: Vec<MudEntry>,
    pub cheese: Vec<Coordinates>,
    pub player1_start: Coordinates,
    pub player2_start: Coordinates,
    pub controlled_players: Vec<Player>,
    pub timing: TimingMode,
    pub move_timeout_ms: u32,
    pub preprocessing_timeout_ms: u32,
}

// ── Game over ──────────────────────────────────────

/// Game-over result data sent to bots at the end of a match.
#[derive(Debug, Clone)]
pub struct GameOver {
    pub result: GameResult,
    pub player1_score: f32,
    pub player2_score: f32,
}

// ── Turn state ──────────────────────────────────────

/// Game position state sent to bots each turn.
///
/// Contains the raw game-position fields. Does **not** include `state_hash`,
/// which is a derived value. Use [`HashedTurnState`] to pair a turn state with
/// a fingerprint of its fields.
#[derive(Debug, Clone)]
pub struct TurnState {
    pub turn: u16,
    pub player1_position: Coordinates,
    pub player2_position: Coordinates,
    pub player1_score: f32,
    pub player2_score: f32,
    pub player1_mud_turns: u8,
    pub player2_mud_turns: u8,
    pub cheese: Vec<Coordinates>,
    pub player1_last_move: Direction,
    pub player2_last_move: Direction,
}

/// An [`TurnState`] paired with a 64-bit fingerprint of its
/// position-defining fields.
///
/// The hash is computed once at construction time. Two states that a bot would
/// analyze differently will hash differently. The hash is not collision-resistant;
/// it's a correlation tag, not a trust boundary.
#[derive(Debug, Clone)]
pub struct HashedTurnState {
    inner: TurnState,
    state_hash: u64,
}

impl HashedTurnState {
    /// Wrap a turn state, computing the hash from its fields.
    pub fn new(ts: TurnState) -> Self {
        let state_hash = Self::compute_hash(&ts);
        Self {
            inner: ts,
            state_hash,
        }
    }

    /// Wrap a turn state with a hash supplied by the producer, without
    /// recomputing it.
    ///
    /// The caller vouches that `state_hash` matches the fields in `ts`. The
    /// wrapper does not verify this. Name chosen to advertise the trust:
    /// mismatches are only caught by the setup-phase hash handshake in
    /// consumers.
    pub fn with_unverified_hash(ts: TurnState, state_hash: u64) -> Self {
        Self {
            inner: ts,
            state_hash,
        }
    }

    /// The fingerprint hash for this turn state.
    pub fn state_hash(&self) -> u64 {
        self.state_hash
    }

    /// Consume the wrapper and return the inner turn state.
    ///
    /// Use `state_hash()` before calling this if you need the hash.
    pub fn into_inner(self) -> TurnState {
        self.inner
    }

    /// Deterministic hash of all game-position fields.
    ///
    /// Two states that a bot would analyze differently must hash differently.
    /// If you add a field to [`TurnState`], update this function.
    fn compute_hash(ts: &TurnState) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        ts.turn.hash(&mut h);
        ts.player1_position.hash(&mut h);
        ts.player2_position.hash(&mut h);
        // Hash scores as half-point u16 to avoid float instability
        ((ts.player1_score * 2.0) as u16).hash(&mut h);
        ((ts.player2_score * 2.0) as u16).hash(&mut h);
        ts.player1_mud_turns.hash(&mut h);
        ts.player2_mud_turns.hash(&mut h);
        ts.cheese.hash(&mut h);
        ts.player1_last_move.hash(&mut h);
        ts.player2_last_move.hash(&mut h);
        h.finish()
    }
}

impl Deref for HashedTurnState {
    type Target = TurnState;

    fn deref(&self) -> &TurnState {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_turn_state() -> TurnState {
        TurnState {
            turn: 42,
            player1_position: Coordinates::new(10, 7),
            player2_position: Coordinates::new(0, 0),
            player1_score: 3.0,
            player2_score: 2.5,
            player1_mud_turns: 0,
            player2_mud_turns: 2,
            cheese: vec![Coordinates::new(5, 5), Coordinates::new(15, 10)],
            player1_last_move: Direction::Up,
            player2_last_move: Direction::Right,
        }
    }

    #[test]
    fn hashed_turn_state_new_computes_deterministic_hash() {
        let a = HashedTurnState::new(sample_turn_state());
        let b = HashedTurnState::new(sample_turn_state());
        assert_eq!(a.state_hash(), b.state_hash());
    }

    /// Pinned hash for `sample_turn_state()` — catches silent changes to the
    /// hasher, field ordering, or score quantization. If this test fails after
    /// an intentional change, recompute the expected value once and update it.
    #[test]
    fn hashed_turn_state_golden_value() {
        let got = HashedTurnState::new(sample_turn_state()).state_hash();
        assert_eq!(
            got, 0x7006_86d9_8688_95f5,
            "update expected value if the hash algorithm changed intentionally"
        );
    }

    #[test]
    fn hashed_turn_state_with_unverified_hash_stores_provided_hash() {
        let ts = sample_turn_state();
        let hts = HashedTurnState::with_unverified_hash(ts, 0xDEAD_BEEF);
        assert_eq!(hts.state_hash(), 0xDEAD_BEEF);
    }

    #[test]
    fn different_states_produce_different_hashes() {
        let ts_a = sample_turn_state();
        let mut ts_b = sample_turn_state();
        ts_b.player1_position = Coordinates::new(5, 5);

        let a = HashedTurnState::new(ts_a);
        let b = HashedTurnState::new(ts_b);
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn deref_accesses_inner_fields() {
        let hts = HashedTurnState::new(sample_turn_state());
        assert_eq!(hts.turn, 42);
        assert_eq!(hts.player1_position, Coordinates::new(10, 7));
    }

    #[test]
    fn direction_conversion_roundtrip() {
        use pyrat_wire::Direction as WireDir;

        for (w, e) in [
            (WireDir::Up, Direction::Up),
            (WireDir::Right, Direction::Right),
            (WireDir::Down, Direction::Down),
            (WireDir::Left, Direction::Left),
            (WireDir::Stay, Direction::Stay),
        ] {
            assert_eq!(wire_to_engine_direction(w), e);
            assert_eq!(engine_to_wire_direction(e), w);
        }
    }

    /// Verify that `Coordinates` hashes identically to the `(u8, u8)` tuple
    /// it replaced, and engine `Direction` hashes identically to the raw `u8`
    /// discriminant used previously. This ensures no hash compatibility break.
    #[test]
    fn hash_compatibility_with_old_tuple_representation() {
        use std::collections::hash_map::DefaultHasher;

        // Coordinates vs (u8, u8)
        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        Coordinates::new(10, 7).hash(&mut h1);
        (10u8, 7u8).hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());

        // Engine Direction vs raw u8 discriminant
        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        Direction::Up.hash(&mut h1);
        0u8.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }
}
