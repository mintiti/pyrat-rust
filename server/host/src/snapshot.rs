//! Reconstruct an engine [`GameState`] from a `(MatchConfig, TurnState)`
//! snapshot. Foundation F4 — the rebuilt state's Zobrist is recomputed so
//! [`GameState::state_hash`] reflects the injected fields, not the
//! builder's initial state.
//!
//! Two consumers share this path:
//! - [`Match::start_turn_with`](crate::match_host::Match) injects an
//!   arbitrary state for analysis-mode play.
//! - [`EmbeddedPlayer`](crate::player::EmbeddedPlayer) rebuilds its bot-side
//!   mirror on `GoState` and `FullState`.
//!
//! The function returns `Result<_, String>`. Callers wrap the message in
//! whatever error variant fits their layer.

use std::collections::HashMap;

use pyrat::{Coordinates, GameBuilder, GameState, MudMap};
use pyrat_protocol::{MatchConfig, TurnState};

/// Construct a fresh engine `GameState` from a `MatchConfig`. The resulting
/// state has the builder's initial player positions and full cheese set.
pub fn build_engine_state(cfg: &MatchConfig) -> Result<GameState, String> {
    let mut walls: HashMap<Coordinates, Vec<Coordinates>> = HashMap::new();
    for (a, b) in &cfg.walls {
        walls.entry(*a).or_default().push(*b);
        walls.entry(*b).or_default().push(*a);
    }
    let mut mud = MudMap::new();
    for entry in &cfg.mud {
        mud.insert(entry.pos1, entry.pos2, entry.turns);
    }

    GameBuilder::new(cfg.width, cfg.height)
        .with_max_turns(cfg.max_turns)
        .with_custom_maze(walls, mud)
        .with_custom_positions(cfg.player1_start, cfg.player2_start)
        .with_custom_cheese(cfg.cheese.clone())
        .build()
        .create(None)
        .map_err(|e| format!("invalid match config: {e}"))
}

/// Rebuild a `GameState` to match an injected `(MatchConfig, TurnState)`
/// snapshot. Field-mutates after [`build_engine_state`], then recomputes the
/// Zobrist (F4) so downstream `state_hash` checks see the post-mutation
/// state.
pub fn rebuild_engine_state(cfg: &MatchConfig, ts: &TurnState) -> Result<GameState, String> {
    let mut game = build_engine_state(cfg)?;
    game.turn = ts.turn;
    game.player1.current_pos = ts.player1_position;
    game.player2.current_pos = ts.player2_position;
    game.player1.score = ts.player1_score;
    game.player2.score = ts.player2_score;
    game.player1.mud_timer = ts.player1_mud_turns;
    game.player2.mud_timer = ts.player2_mud_turns;
    // Cheese: drop any cells the builder placed that aren't in `ts.cheese`.
    // `CheeseBoard::clear` isn't exposed; we toggle by reading the cfg's
    // initial cheese (= what the builder placed) and removing absences.
    for pos in cfg.cheese.iter() {
        if !ts.cheese.contains(pos) {
            game.cheese.take_cheese(*pos);
        }
    }
    game.recompute_state_hash();
    Ok(game)
}
