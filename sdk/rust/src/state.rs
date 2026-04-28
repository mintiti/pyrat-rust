//! Game state with perspective mapping and convenience methods.
//!
//! [`GameState`] owns a real engine `GameState` mirror. The host advances its
//! canonical state turn-by-turn; the SDK applies the same Advance deltas so
//! the two stay in sync. Verification is via `state_hash()` (Zobrist), which
//! the engine maintains incrementally on `process_turn`.
//!
//! Perspective mapping translates player1/player2 to my/opponent based on the
//! slot assigned by `HostMsg::Welcome` during the handshake.

use std::collections::HashMap;

use pyrat::{Coordinates, Direction, GameBuilder, MudMap};
use pyrat_engine_interface::pathfinding::FullPathResult;
use pyrat_engine_interface::Maze;
use pyrat_wire::Player;

use crate::GameSim;
use pyrat_protocol::{MatchConfig, TurnState};

/// SDK-facing game state. Built from `MatchConfig`, advanced each turn.
pub struct GameState {
    engine: pyrat::GameState,
    my_player: Player,

    // The protocol carries last_move per player; engine GameState doesn't track it.
    player1_last_move: Direction,
    player2_last_move: Direction,

    // Static config from MatchConfig (not part of the engine state).
    move_timeout_ms: u32,
    preprocessing_timeout_ms: u32,
}

impl GameState {
    /// Build from match configuration received during setup.
    ///
    /// `slot` is the player slot assigned by [`HostMsg::Welcome`]. Everything
    /// from `cfg.controlled_players` is ignored — the slot is authoritative.
    pub fn from_config(slot: Player, cfg: &MatchConfig) -> Result<Self, String> {
        let engine = build_engine(cfg)?;
        Ok(Self {
            engine,
            my_player: slot,
            player1_last_move: Direction::Stay,
            player2_last_move: Direction::Stay,
            move_timeout_ms: cfg.move_timeout_ms,
            preprocessing_timeout_ms: cfg.preprocessing_timeout_ms,
        })
    }

    /// Apply an Advance delta. Updates the engine via `process_turn`, which
    /// keeps `state_hash` consistent via Zobrist deltas. Returns the new hash.
    pub fn apply_advance(&mut self, p1: Direction, p2: Direction) -> u64 {
        self.engine.process_turn(p1, p2);
        self.player1_last_move = p1;
        self.player2_last_move = p2;
        self.engine.state_hash()
    }

    /// Reload the engine from a `TurnState` (received via GoState or FullState).
    /// Recomputes the hash from scratch and returns it.
    pub fn load_turn_state(&mut self, ts: &TurnState) -> u64 {
        apply_turn_state(&mut self.engine, ts);
        self.player1_last_move = ts.player1_last_move;
        self.player2_last_move = ts.player2_last_move;
        self.engine.state_hash()
    }

    /// Reload from a fresh `MatchConfig` and `TurnState` (FullState recovery).
    /// Rebuilds the engine and recomputes the hash. Returns the new hash.
    pub fn load_full_state(&mut self, cfg: &MatchConfig, ts: &TurnState) -> Result<u64, String> {
        self.engine = build_engine(cfg)?;
        self.move_timeout_ms = cfg.move_timeout_ms;
        self.preprocessing_timeout_ms = cfg.preprocessing_timeout_ms;
        Ok(self.load_turn_state(ts))
    }

    // ── Perspective helpers ─────────────────────────

    fn pick<T: Copy>(&self, p1: T, p2: T) -> T {
        match self.my_player {
            Player::Player1 => p1,
            _ => p2,
        }
    }

    fn pick_opponent<T: Copy>(&self, p1: T, p2: T) -> T {
        match self.my_player {
            Player::Player1 => p2,
            _ => p1,
        }
    }

    // ── Perspective-mapped accessors ─────────────────

    /// Which player this bot controls (assigned by `Welcome`).
    pub fn my_player(&self) -> Player {
        self.my_player
    }

    pub fn my_position(&self) -> Coordinates {
        self.pick(self.player1_position(), self.player2_position())
    }

    pub fn opponent_position(&self) -> Coordinates {
        self.pick_opponent(self.player1_position(), self.player2_position())
    }

    pub fn my_score(&self) -> f32 {
        self.pick(self.player1_score(), self.player2_score())
    }

    pub fn opponent_score(&self) -> f32 {
        self.pick_opponent(self.player1_score(), self.player2_score())
    }

    pub fn my_mud_turns(&self) -> u8 {
        self.pick(self.player1_mud_turns(), self.player2_mud_turns())
    }

    pub fn opponent_mud_turns(&self) -> u8 {
        self.pick_opponent(self.player1_mud_turns(), self.player2_mud_turns())
    }

    pub fn my_last_move(&self) -> Direction {
        self.pick(self.player1_last_move, self.player2_last_move)
    }

    pub fn opponent_last_move(&self) -> Direction {
        self.pick_opponent(self.player1_last_move, self.player2_last_move)
    }

    // ── Raw (objective) accessors ────────────────────

    pub fn player1_position(&self) -> Coordinates {
        self.engine.player1.current_pos
    }

    pub fn player2_position(&self) -> Coordinates {
        self.engine.player2.current_pos
    }

    pub fn player1_score(&self) -> f32 {
        self.engine.player1.score
    }

    pub fn player2_score(&self) -> f32 {
        self.engine.player2.score
    }

    pub fn player1_mud_turns(&self) -> u8 {
        self.engine.player1.mud_timer
    }

    pub fn player2_mud_turns(&self) -> u8 {
        self.engine.player2.mud_timer
    }

    pub fn player1_last_move(&self) -> Direction {
        self.player1_last_move
    }

    pub fn player2_last_move(&self) -> Direction {
        self.player2_last_move
    }

    pub fn turn(&self) -> u16 {
        self.engine.turn
    }

    pub fn max_turns(&self) -> u16 {
        self.engine.max_turns
    }

    pub fn cheese(&self) -> Vec<Coordinates> {
        self.engine.cheese_positions()
    }

    pub fn move_timeout_ms(&self) -> u32 {
        self.move_timeout_ms
    }

    pub fn preprocessing_timeout_ms(&self) -> u32 {
        self.preprocessing_timeout_ms
    }

    pub fn state_hash(&self) -> u64 {
        self.engine.state_hash()
    }

    // ── Convenience (delegate to a borrowed Maze) ────

    pub fn width(&self) -> u8 {
        self.engine.width
    }

    pub fn height(&self) -> u8 {
        self.engine.height
    }

    fn maze(&self) -> Maze<'_> {
        Maze::new(
            &self.engine.move_table,
            &self.engine.mud,
            self.engine.width,
            self.engine.height,
        )
    }

    /// Directions from `pos` that don't hit a wall or boundary.
    /// Defaults to `my_position()` if `pos` is `None`.
    pub fn effective_moves(&self, pos: Option<Coordinates>) -> Vec<Direction> {
        self.maze()
            .effective_moves(pos.unwrap_or_else(|| self.my_position()))
    }

    /// Cost (in turns) of moving in `dir` from `pos`.
    /// Defaults to `my_position()` if `pos` is `None`.
    pub fn move_cost(&self, dir: Direction, pos: Option<Coordinates>) -> Option<u8> {
        self.maze()
            .move_cost(pos.unwrap_or_else(|| self.my_position()), dir)
    }

    /// Shortest path with full direction sequence between two cells.
    pub fn shortest_path(&self, from: Coordinates, to: Coordinates) -> Option<FullPathResult> {
        pyrat_engine_interface::shortest_path_full(from, to, &self.maze())
    }

    /// Nearest cheese from `pos`. Returns the full path to the closest cheese.
    /// Defaults to `my_position()` if `pos` is `None`.
    ///
    /// When multiple cheeses tie at the minimum distance, returns the first one
    /// in the cheese list. Use [`nearest_cheeses`](Self::nearest_cheeses) to get
    /// all tied results.
    pub fn nearest_cheese(&self, pos: Option<Coordinates>) -> Option<FullPathResult> {
        self.nearest_cheeses(pos).into_iter().next()
    }

    /// All cheeses tied at the minimum distance from `pos`, each with a full
    /// direction sequence. Defaults to `my_position()` if `pos` is `None`.
    pub fn nearest_cheeses(&self, pos: Option<Coordinates>) -> Vec<FullPathResult> {
        let from = pos.unwrap_or_else(|| self.my_position());
        let cheese = self.engine.cheese_positions();
        pyrat_engine_interface::nearest_cheeses_full(from, &cheese, &self.maze())
    }

    /// Weighted distances from `pos` to all reachable cells.
    /// Defaults to `my_position()` if `pos` is `None`.
    pub fn distances_from(&self, pos: Option<Coordinates>) -> HashMap<Coordinates, u32> {
        pyrat_engine_interface::distances_from(
            pos.unwrap_or_else(|| self.my_position()),
            &self.maze(),
        )
    }

    /// Clone the game into a mutable simulation state.
    pub fn to_sim(&self) -> GameSim {
        self.engine.clone()
    }
}

fn build_engine(cfg: &MatchConfig) -> Result<pyrat::GameState, String> {
    let mut wall_map: HashMap<Coordinates, Vec<Coordinates>> = HashMap::new();
    for (a, b) in &cfg.walls {
        wall_map.entry(*a).or_default().push(*b);
        wall_map.entry(*b).or_default().push(*a);
    }

    let mut mud_map = MudMap::new();
    for m in &cfg.mud {
        mud_map.insert(m.pos1, m.pos2, m.turns);
    }

    GameBuilder::new(cfg.width, cfg.height)
        .with_max_turns(cfg.max_turns)
        .with_custom_maze(wall_map, mud_map)
        .with_custom_positions(cfg.player1_start, cfg.player2_start)
        .with_custom_cheese(cfg.cheese.clone())
        .build()
        .create(None)
        .map_err(|e| e.to_string())
}

fn apply_turn_state(engine: &mut pyrat::GameState, ts: &TurnState) {
    engine.player1.current_pos = ts.player1_position;
    engine.player1.score = ts.player1_score;
    engine.player1.mud_timer = ts.player1_mud_turns;

    engine.player2.current_pos = ts.player2_position;
    engine.player2.score = ts.player2_score;
    engine.player2.mud_timer = ts.player2_mud_turns;

    engine.turn = ts.turn;

    // Reset cheese to the TurnState's set: take any not present in `ts.cheese`.
    for pos in engine.cheese_positions() {
        if !ts.cheese.contains(&pos) {
            engine.cheese.take_cheese(pos);
        }
    }

    engine.recompute_state_hash();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat_protocol::TurnState;
    use pyrat_wire::TimingMode;

    fn test_config() -> MatchConfig {
        MatchConfig {
            width: 5,
            height: 5,
            max_turns: 300,
            walls: vec![],
            mud: vec![],
            cheese: vec![Coordinates::new(2, 2), Coordinates::new(4, 4)],
            player1_start: Coordinates::new(0, 0),
            player2_start: Coordinates::new(4, 4),
            controlled_players: vec![],
            timing: TimingMode::Wait,
            move_timeout_ms: 1000,
            preprocessing_timeout_ms: 5000,
        }
    }

    fn test_turn_state() -> TurnState {
        TurnState {
            turn: 5,
            player1_position: Coordinates::new(1, 1),
            player2_position: Coordinates::new(3, 3),
            player1_score: 1.0,
            player2_score: 0.5,
            player1_mud_turns: 0,
            player2_mud_turns: 2,
            cheese: vec![Coordinates::new(4, 4)],
            player1_last_move: Direction::Right,
            player2_last_move: Direction::Left,
        }
    }

    #[test]
    fn from_config_starts_at_turn_zero() {
        let cfg = test_config();
        let state = GameState::from_config(Player::Player1, &cfg).unwrap();

        assert_eq!(state.turn(), 0);
        assert_eq!(state.my_position(), Coordinates::new(0, 0));
        assert_eq!(state.opponent_position(), Coordinates::new(4, 4));
        assert_eq!(state.cheese().len(), 2);
    }

    #[test]
    fn load_turn_state_replaces_dynamic_fields() {
        let cfg = test_config();
        let mut state = GameState::from_config(Player::Player1, &cfg).unwrap();
        state.load_turn_state(&test_turn_state());

        assert_eq!(state.turn(), 5);
        assert_eq!(state.my_position(), Coordinates::new(1, 1));
        assert_eq!(state.opponent_position(), Coordinates::new(3, 3));
        assert!((state.my_score() - 1.0).abs() < f32::EPSILON);
        assert!((state.opponent_score() - 0.5).abs() < f32::EPSILON);
        assert_eq!(state.my_mud_turns(), 0);
        assert_eq!(state.opponent_mud_turns(), 2);
        assert_eq!(state.my_last_move(), Direction::Right);
        assert_eq!(state.opponent_last_move(), Direction::Left);
        assert_eq!(state.cheese().len(), 1);
    }

    #[test]
    fn perspective_player2() {
        let cfg = test_config();
        let mut state = GameState::from_config(Player::Player2, &cfg).unwrap();
        state.load_turn_state(&test_turn_state());

        assert_eq!(state.my_player(), Player::Player2);
        assert_eq!(state.my_position(), Coordinates::new(3, 3));
        assert_eq!(state.opponent_position(), Coordinates::new(1, 1));
        assert!((state.my_score() - 0.5).abs() < f32::EPSILON);
        assert!((state.opponent_score() - 1.0).abs() < f32::EPSILON);
        assert_eq!(state.my_mud_turns(), 2);
        assert_eq!(state.opponent_mud_turns(), 0);
    }

    #[test]
    fn raw_accessors() {
        let mut state = GameState::from_config(Player::Player1, &test_config()).unwrap();
        state.load_turn_state(&test_turn_state());

        assert_eq!(state.player1_position(), Coordinates::new(1, 1));
        assert_eq!(state.player2_position(), Coordinates::new(3, 3));
        assert!((state.player1_score() - 1.0).abs() < f32::EPSILON);
        assert!((state.player2_score() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_moves_from_corner() {
        let state = GameState::from_config(Player::Player1, &test_config()).unwrap();
        let moves = state.effective_moves(None);
        assert!(moves.contains(&Direction::Right));
        assert!(moves.contains(&Direction::Up));
        assert!(!moves.contains(&Direction::Left));
        assert!(!moves.contains(&Direction::Down));
    }

    #[test]
    fn nearest_cheese_works() {
        let state = GameState::from_config(Player::Player1, &test_config()).unwrap();
        let result = state.nearest_cheese(None).unwrap();
        // From (0,0), nearest cheese is (2,2) at distance 4.
        assert_eq!(result.target, Coordinates::new(2, 2));
        assert_eq!(result.cost, 4);
        assert!(!result.path.is_empty());
    }

    #[test]
    fn to_sim_make_unmake() {
        let state = GameState::from_config(Player::Player1, &test_config()).unwrap();
        let mut sim = state.to_sim();

        let p1_before = sim.player1_position();
        let undo = sim.make_move(Direction::Right, Direction::Stay);
        assert_ne!(sim.player1_position(), p1_before);

        sim.unmake_move(undo);
        assert_eq!(sim.player1_position(), p1_before);
    }

    #[test]
    fn to_sim_reflects_loaded_state() {
        let mut state = GameState::from_config(Player::Player1, &test_config()).unwrap();
        state.load_turn_state(&test_turn_state());

        let sim = state.to_sim();
        assert_eq!(sim.player1_position(), Coordinates::new(1, 1));
        assert_eq!(sim.player2_position(), Coordinates::new(3, 3));
        assert!((sim.player1_score() - 1.0).abs() < f32::EPSILON);
        assert_eq!(sim.turn, 5);
        assert_eq!(sim.cheese_positions().len(), 1);
    }

    #[test]
    fn to_sim_preserves_total_cheese() {
        let cfg = test_config();
        let mut state = GameState::from_config(Player::Player1, &cfg).unwrap();
        let original_cheese_count = cfg.cheese.len();

        // Mid-game: one cheese collected, p1 has score.
        state.load_turn_state(&TurnState {
            turn: 10,
            player1_position: Coordinates::new(2, 2),
            player2_position: Coordinates::new(0, 0),
            player1_score: 1.0,
            player2_score: 0.0,
            player1_mud_turns: 0,
            player2_mud_turns: 0,
            cheese: vec![Coordinates::new(4, 4)],
            player1_last_move: Direction::Stay,
            player2_last_move: Direction::Stay,
        });

        let sim = state.to_sim();
        assert_eq!(
            sim.cheese.total_cheese() as usize,
            original_cheese_count,
            "load_turn_state must preserve initial_cheese_count"
        );
        assert_eq!(sim.cheese_positions().len(), 1);
        assert!(
            !sim.check_game_over(),
            "score 1.0 doesn't exceed half of 2 total cheese"
        );
    }

    #[test]
    fn distances_from_works() {
        let state = GameState::from_config(Player::Player1, &test_config()).unwrap();
        let dists = state.distances_from(Some(Coordinates::new(0, 0)));
        assert_eq!(dists[&Coordinates::new(0, 0)], 0);
        assert_eq!(dists[&Coordinates::new(1, 0)], 1);
        assert_eq!(dists[&Coordinates::new(4, 4)], 8);
    }

    /// The hash the SDK derives from MatchConfig must match the host's hash
    /// (both engines compute the same Zobrist of the same maze + initial state).
    #[test]
    fn initial_hash_is_stable() {
        let cfg = test_config();
        let a = GameState::from_config(Player::Player1, &cfg).unwrap();
        let b = GameState::from_config(Player::Player1, &cfg).unwrap();
        assert_eq!(a.state_hash(), b.state_hash());
        assert_ne!(a.state_hash(), 0, "initial hash is non-trivial");
    }

    /// `apply_advance` must agree with `load_turn_state` for the same
    /// resulting position — i.e., process_turn's incremental Zobrist update
    /// matches a from-scratch recompute.
    #[test]
    fn apply_advance_matches_load_turn_state() {
        let cfg = MatchConfig {
            width: 5,
            height: 5,
            max_turns: 300,
            walls: vec![],
            mud: vec![],
            cheese: vec![Coordinates::new(2, 2)],
            player1_start: Coordinates::new(0, 0),
            player2_start: Coordinates::new(4, 4),
            controlled_players: vec![],
            timing: TimingMode::Wait,
            move_timeout_ms: 1000,
            preprocessing_timeout_ms: 5000,
        };

        // Path A: advance one turn via process_turn.
        let mut a = GameState::from_config(Player::Player1, &cfg).unwrap();
        let advanced = a.apply_advance(Direction::Right, Direction::Left);

        // Path B: load the equivalent TurnState directly, recompute hash.
        let mut b = GameState::from_config(Player::Player1, &cfg).unwrap();
        let loaded = b.load_turn_state(&TurnState {
            turn: 1,
            player1_position: Coordinates::new(1, 0),
            player2_position: Coordinates::new(3, 4),
            player1_score: 0.0,
            player2_score: 0.0,
            player1_mud_turns: 0,
            player2_mud_turns: 0,
            cheese: vec![Coordinates::new(2, 2)],
            player1_last_move: Direction::Right,
            player2_last_move: Direction::Left,
        });

        assert_eq!(advanced, loaded);
    }
}
