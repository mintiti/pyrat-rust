//! Zobrist hashing for incremental game state hashing.
//!
//! Global tables generated from a deterministic seed at first access.
//! Position split into separate X/Y tables per player (one coord changes per move).
//! Cheese uses flat indexing to avoid rank deficiency.

use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use crate::game::game_logic::GameState;
use crate::game::types::MudMap;
use crate::MoveTable;

const MAX_DIM: usize = 64;
const MAX_MUD: usize = 16;
const MAX_SCORE_X2: usize = 512;
const MAX_TURNS: usize = 1024;
const MAX_CELLS: usize = MAX_DIM * MAX_DIM;

const SEED: u64 = 0x5079_5261_745a_6f62; // "PyRatZob" in hex

struct ZobristKeys {
    pos_x: [[u64; MAX_DIM]; 2],
    pos_y: [[u64; MAX_DIM]; 2],
    mud: [[u64; MAX_MUD]; 2],
    score: [[u64; MAX_SCORE_X2]; 2],
    turn: [u64; MAX_TURNS],
    cheese: [u64; MAX_CELLS],
}

static KEYS: LazyLock<ZobristKeys> = LazyLock::new(|| {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut gen = || rng.random::<u64>();

    let mut pos_x = [[0u64; MAX_DIM]; 2];
    let mut pos_y = [[0u64; MAX_DIM]; 2];
    let mut mud = [[0u64; MAX_MUD]; 2];
    let mut score = [[0u64; MAX_SCORE_X2]; 2];
    let mut turn = [0u64; MAX_TURNS];
    let mut cheese = [0u64; MAX_CELLS];

    for p in 0..2 {
        for v in &mut pos_x[p] {
            *v = gen();
        }
        for v in &mut pos_y[p] {
            *v = gen();
        }
        for v in &mut mud[p] {
            *v = gen();
        }
        for v in &mut score[p] {
            *v = gen();
        }
    }
    for v in &mut turn {
        *v = gen();
    }
    for v in &mut cheese {
        *v = gen();
    }

    ZobristKeys {
        pos_x,
        pos_y,
        mud,
        score,
        turn,
        cheese,
    }
});

/// XOR delta for a player's position and mud changes.
#[inline(always)]
pub fn player_delta(
    player: usize,
    old_x: u8,
    old_y: u8,
    old_mud: u8,
    new_x: u8,
    new_y: u8,
    new_mud: u8,
) -> u64 {
    let k = &KEYS;
    let mut h = 0u64;
    if old_x != new_x {
        h ^= k.pos_x[player][old_x as usize] ^ k.pos_x[player][new_x as usize];
    }
    if old_y != new_y {
        h ^= k.pos_y[player][old_y as usize] ^ k.pos_y[player][new_y as usize];
    }
    if old_mud != new_mud {
        h ^= k.mud[player][old_mud as usize] ^ k.mud[player][new_mud as usize];
    }
    h
}

/// XOR delta for a player's score change.
#[inline(always)]
pub fn score_delta(player: usize, old_x2: u16, new_x2: u16) -> u64 {
    let k = &KEYS;
    k.score[player][old_x2 as usize] ^ k.score[player][new_x2 as usize]
}

/// XOR delta for a turn change.
#[inline(always)]
pub fn turn_delta(old: u16, new: u16) -> u64 {
    let k = &KEYS;
    k.turn[old as usize] ^ k.turn[new as usize]
}

/// Zobrist hash for a single cheese cell (toggle on/off).
#[inline(always)]
pub fn cheese_hash(x: u8, y: u8) -> u64 {
    let k = &KEYS;
    k.cheese[x as usize * MAX_DIM + y as usize]
}

/// Full Zobrist hash computed from scratch. Used for initialization and debug verification.
pub fn compute_from_scratch(state: &GameState) -> u64 {
    let k = &KEYS;
    let mut h = 0u64;

    // Player positions
    h ^= k.pos_x[0][state.player1.current_pos.x as usize];
    h ^= k.pos_y[0][state.player1.current_pos.y as usize];
    h ^= k.pos_x[1][state.player2.current_pos.x as usize];
    h ^= k.pos_y[1][state.player2.current_pos.y as usize];

    // Mud timers
    h ^= k.mud[0][state.player1.mud_timer as usize];
    h ^= k.mud[1][state.player2.mud_timer as usize];

    // Scores (f32 → u16 via ×2)
    h ^= k.score[0][(state.player1.score * 2.0) as usize];
    h ^= k.score[1][(state.player2.score * 2.0) as usize];

    // Turn
    h ^= k.turn[state.turn as usize];

    // Cheese (iterate set bits)
    for (word_idx, &word) in state.cheese.bits().iter().enumerate() {
        let mut w = word;
        while w != 0 {
            let bit = w.trailing_zeros() as usize;
            let flat_idx = word_idx * 64 + bit;
            // Convert flat index (row-major: y*width+x) to our x*MAX_DIM+y format
            let x = flat_idx % state.width as usize;
            let y = flat_idx / state.width as usize;
            h ^= k.cheese[x * MAX_DIM + y];
            w &= w - 1;
        }
    }

    h
}

/// SipHash over static topology. Called once at game creation, XOR'd into the Zobrist hash
/// so that identical dynamic states on different mazes produce different hashes.
pub fn maze_hash(move_table: &MoveTable, mud: &MudMap, width: u8, height: u8) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    move_table.bytes().hash(&mut hasher);
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    // Hash mud entries in a deterministic order
    let mut mud_entries: Vec<_> = mud.iter().collect();
    mud_entries.sort_unstable();
    mud_entries.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_initialized() {
        // Just force initialization and check non-zero
        let k = &*KEYS;
        assert_ne!(k.pos_x[0][0], 0);
        assert_ne!(k.turn[0], 0);
        assert_ne!(k.cheese[0], 0);
    }
}
