//! Game state with perspective mapping and convenience methods.
//!
//! [`GameState`] wraps a [`GameView`] (static maze topology) plus per-turn
//! dynamic data. Perspective mapping translates player1/player2 to my/opponent
//! based on `controlled_players[0]`.

use std::collections::HashMap;

use pyrat::{Coordinates, Direction};
use pyrat_engine_interface::pathfinding::FullPathResult;
use pyrat_engine_interface::GameView;
use pyrat_wire::Player;

use crate::GameSim;
use pyrat_protocol::{HashedTurnState, OwnedMatchConfig, OwnedTurnState};

/// SDK-facing game state. Built once from `OwnedMatchConfig`, updated each turn.
pub struct GameState {
    view: GameView,
    my_player: Player,
    controlled_players: Vec<Player>,

    // Per-turn dynamic state
    turn: u16,
    player1_position: Coordinates,
    player2_position: Coordinates,
    player1_score: f32,
    player2_score: f32,
    player1_mud_turns: u8,
    player2_mud_turns: u8,
    player1_last_move: Direction,
    player2_last_move: Direction,
    cheese: Vec<Coordinates>,
    state_hash: u64,

    // Static config
    max_turns: u16,
    move_timeout_ms: u32,
    preprocessing_timeout_ms: u32,
}

impl GameState {
    /// Build from match configuration received during setup.
    pub fn from_config(cfg: &OwnedMatchConfig) -> Result<Self, String> {
        let walls: Vec<(Coordinates, Coordinates)> = cfg.walls.clone();
        let mud: Vec<(Coordinates, Coordinates, u8)> =
            cfg.mud.iter().map(|m| (m.pos1, m.pos2, m.turns)).collect();

        let view = GameView::from_config(
            cfg.width,
            cfg.height,
            cfg.max_turns,
            &walls,
            &mud,
            cfg.cheese.clone(),
            cfg.player1_start,
            cfg.player2_start,
        )?;

        let my_player = cfg
            .controlled_players
            .first()
            .copied()
            .unwrap_or(Player::Player1);

        Ok(Self {
            view,
            my_player,
            controlled_players: cfg.controlled_players.clone(),
            turn: 0,
            player1_position: cfg.player1_start,
            player2_position: cfg.player2_start,
            player1_score: 0.0,
            player2_score: 0.0,
            player1_mud_turns: 0,
            player2_mud_turns: 0,
            player1_last_move: Direction::Stay,
            player2_last_move: Direction::Stay,
            cheese: cfg.cheese.clone(),
            state_hash: 0,
            max_turns: cfg.max_turns,
            move_timeout_ms: cfg.move_timeout_ms,
            preprocessing_timeout_ms: cfg.preprocessing_timeout_ms,
        })
    }

    /// Update dynamic state from a TurnState message.
    pub fn update(&mut self, hts: HashedTurnState) {
        let hash = hts.state_hash();
        let ts = hts.into_inner();
        self.turn = ts.turn;
        self.player1_position = ts.player1_position;
        self.player2_position = ts.player2_position;
        self.player1_score = ts.player1_score;
        self.player2_score = ts.player2_score;
        self.player1_mud_turns = ts.player1_mud_turns;
        self.player2_mud_turns = ts.player2_mud_turns;
        self.player1_last_move = ts.player1_last_move;
        self.player2_last_move = ts.player2_last_move;
        self.cheese = ts.cheese;
        self.state_hash = hash;
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

    /// Which player this bot controls (first in controlled_players).
    pub fn my_player(&self) -> Player {
        self.my_player
    }

    /// All controlled players (usually just one, two for hivemind).
    pub fn controlled_players(&self) -> &[Player] {
        &self.controlled_players
    }

    pub fn my_position(&self) -> Coordinates {
        self.pick(self.player1_position, self.player2_position)
    }

    pub fn opponent_position(&self) -> Coordinates {
        self.pick_opponent(self.player1_position, self.player2_position)
    }

    pub fn my_score(&self) -> f32 {
        self.pick(self.player1_score, self.player2_score)
    }

    pub fn opponent_score(&self) -> f32 {
        self.pick_opponent(self.player1_score, self.player2_score)
    }

    pub fn my_mud_turns(&self) -> u8 {
        self.pick(self.player1_mud_turns, self.player2_mud_turns)
    }

    pub fn opponent_mud_turns(&self) -> u8 {
        self.pick_opponent(self.player1_mud_turns, self.player2_mud_turns)
    }

    pub fn my_last_move(&self) -> Direction {
        self.pick(self.player1_last_move, self.player2_last_move)
    }

    pub fn opponent_last_move(&self) -> Direction {
        self.pick_opponent(self.player1_last_move, self.player2_last_move)
    }

    // ── Raw (objective) accessors ────────────────────

    pub fn player1_position(&self) -> Coordinates {
        self.player1_position
    }

    pub fn player2_position(&self) -> Coordinates {
        self.player2_position
    }

    pub fn player1_score(&self) -> f32 {
        self.player1_score
    }

    pub fn player2_score(&self) -> f32 {
        self.player2_score
    }

    pub fn player1_mud_turns(&self) -> u8 {
        self.player1_mud_turns
    }

    pub fn player2_mud_turns(&self) -> u8 {
        self.player2_mud_turns
    }

    pub fn player1_last_move(&self) -> Direction {
        self.player1_last_move
    }

    pub fn player2_last_move(&self) -> Direction {
        self.player2_last_move
    }

    pub fn turn(&self) -> u16 {
        self.turn
    }

    pub fn max_turns(&self) -> u16 {
        self.max_turns
    }

    pub fn cheese(&self) -> &[Coordinates] {
        &self.cheese
    }

    pub fn move_timeout_ms(&self) -> u32 {
        self.move_timeout_ms
    }

    pub fn preprocessing_timeout_ms(&self) -> u32 {
        self.preprocessing_timeout_ms
    }

    pub fn state_hash(&self) -> u64 {
        self.state_hash
    }

    /// Set the state hash (used after verification in setup phase).
    pub(crate) fn set_state_hash(&mut self, hash: u64) {
        self.state_hash = hash;
    }

    /// Compute the initial state hash from turn-0 fields.
    ///
    /// Delegates to `HashedTurnState::new` so the hash algorithm is defined in
    /// one place. The SDK uses this to verify agreement with the host.
    pub fn compute_initial_hash(&self) -> u64 {
        let ts = OwnedTurnState {
            turn: 0,
            player1_position: self.player1_position,
            player2_position: self.player2_position,
            player1_score: 0.0,
            player2_score: 0.0,
            player1_mud_turns: 0,
            player2_mud_turns: 0,
            cheese: self.cheese.clone(),
            player1_last_move: Direction::Stay,
            player2_last_move: Direction::Stay,
        };
        HashedTurnState::new(ts).state_hash()
    }

    // ── Convenience (delegate to GameView/pathfinding) ──

    pub fn width(&self) -> u8 {
        self.view.width()
    }

    pub fn height(&self) -> u8 {
        self.view.height()
    }

    /// Directions from `pos` that don't hit a wall or boundary.
    /// Defaults to `my_position()` if `pos` is `None`.
    pub fn effective_moves(&self, pos: Option<Coordinates>) -> Vec<Direction> {
        self.view
            .effective_moves(pos.unwrap_or_else(|| self.my_position()))
    }

    /// Cost (in turns) of moving in `dir` from `pos`.
    /// Defaults to `my_position()` if `pos` is `None`.
    pub fn move_cost(&self, dir: Direction, pos: Option<Coordinates>) -> Option<u8> {
        self.view
            .move_cost(pos.unwrap_or_else(|| self.my_position()), dir)
    }

    /// Shortest path with full direction sequence between two cells.
    pub fn shortest_path(&self, from: Coordinates, to: Coordinates) -> Option<FullPathResult> {
        pyrat_engine_interface::shortest_path_full(from, to, &self.view.maze())
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
        pyrat_engine_interface::nearest_cheeses_full(from, &self.cheese, &self.view.maze())
    }

    /// Weighted distances from `pos` to all reachable cells.
    /// Defaults to `my_position()` if `pos` is `None`.
    pub fn distances_from(&self, pos: Option<Coordinates>) -> HashMap<Coordinates, u32> {
        self.view
            .distances_from(pos.unwrap_or_else(|| self.my_position()))
    }

    /// Clone the game into a mutable simulation state.
    pub fn to_sim(&self) -> GameSim {
        let mut game = self.view.snapshot();

        // Patch dynamic state to match current turn
        game.player1.current_pos = self.player1_position;
        game.player1.score = self.player1_score;
        game.player1.mud_timer = self.player1_mud_turns;

        game.player2.current_pos = self.player2_position;
        game.player2.score = self.player2_score;
        game.player2.mud_timer = self.player2_mud_turns;

        // Remove collected cheese (preserves initial_cheese_count for win threshold)
        for pos in game.cheese_positions() {
            if !self.cheese.contains(&pos) {
                game.cheese.take_cheese(pos);
            }
        }

        game.turn = self.turn;

        game
    }

    /// Read-only access to the underlying `GameView`.
    pub fn view(&self) -> &GameView {
        &self.view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat_protocol::OwnedTurnState;
    use pyrat_wire::TimingMode;

    fn test_config() -> OwnedMatchConfig {
        OwnedMatchConfig {
            width: 5,
            height: 5,
            max_turns: 300,
            walls: vec![],
            mud: vec![],
            cheese: vec![Coordinates::new(2, 2), Coordinates::new(4, 4)],
            player1_start: Coordinates::new(0, 0),
            player2_start: Coordinates::new(4, 4),
            controlled_players: vec![Player::Player1],
            timing: TimingMode::Wait,
            move_timeout_ms: 1000,
            preprocessing_timeout_ms: 5000,
        }
    }

    fn test_turn_state() -> HashedTurnState {
        HashedTurnState::with_unverified_hash(
            OwnedTurnState {
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
            },
            0xABCD,
        )
    }

    #[test]
    fn compute_initial_hash_matches_hashed_turn_state() {
        let cfg = test_config();
        let state = GameState::from_config(&cfg).unwrap();

        let expected = HashedTurnState::new(OwnedTurnState {
            turn: 0,
            player1_position: cfg.player1_start,
            player2_position: cfg.player2_start,
            player1_score: 0.0,
            player2_score: 0.0,
            player1_mud_turns: 0,
            player2_mud_turns: 0,
            cheese: cfg.cheese.clone(),
            player1_last_move: Direction::Stay,
            player2_last_move: Direction::Stay,
        })
        .state_hash();

        assert_eq!(state.compute_initial_hash(), expected);
    }

    #[test]
    fn from_config_and_update() {
        let cfg = test_config();
        let mut state = GameState::from_config(&cfg).unwrap();

        assert_eq!(state.turn(), 0);
        assert_eq!(state.my_position(), Coordinates::new(0, 0));
        assert_eq!(state.opponent_position(), Coordinates::new(4, 4));
        assert_eq!(state.cheese().len(), 2);

        let ts = test_turn_state();
        state.update(ts);

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
        assert_eq!(state.state_hash(), 0xABCD);
    }

    #[test]
    fn perspective_player2() {
        let mut cfg = test_config();
        cfg.controlled_players = vec![Player::Player2];
        let mut state = GameState::from_config(&cfg).unwrap();

        let ts = test_turn_state();
        state.update(ts);

        // Perspective is flipped
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
        let mut state = GameState::from_config(&test_config()).unwrap();
        state.update(test_turn_state());

        assert_eq!(state.player1_position(), Coordinates::new(1, 1));
        assert_eq!(state.player2_position(), Coordinates::new(3, 3));
        assert!((state.player1_score() - 1.0).abs() < f32::EPSILON);
        assert!((state.player2_score() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_moves() {
        let state = GameState::from_config(&test_config()).unwrap();
        let moves = state.effective_moves(None);
        // From (0,0) on a 5x5 open grid: Right and Up
        assert!(moves.contains(&Direction::Right));
        assert!(moves.contains(&Direction::Up));
        assert!(!moves.contains(&Direction::Left));
        assert!(!moves.contains(&Direction::Down));
    }

    #[test]
    fn nearest_cheese_works() {
        let state = GameState::from_config(&test_config()).unwrap();
        let result = state.nearest_cheese(None);
        assert!(result.is_some());
        let r = result.unwrap();
        // From (0,0), nearest cheese is (2,2) at distance 4
        assert_eq!(r.target, Coordinates::new(2, 2));
        assert_eq!(r.cost, 4);
        assert!(!r.path.is_empty());
    }

    #[test]
    fn to_sim_make_unmake() {
        let state = GameState::from_config(&test_config()).unwrap();
        let mut sim = state.to_sim();

        let p1_before = sim.player1_position();
        let undo = sim.make_move(Direction::Right, Direction::Stay);
        assert_ne!(sim.player1_position(), p1_before);

        sim.unmake_move(undo);
        assert_eq!(sim.player1_position(), p1_before);
    }

    #[test]
    fn to_sim_reflects_current_state() {
        let mut state = GameState::from_config(&test_config()).unwrap();
        state.update(test_turn_state());

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
        let mut state = GameState::from_config(&cfg).unwrap();
        let original_cheese_count = cfg.cheese.len();

        // Simulate mid-game: one cheese collected, player has some score
        state.update(HashedTurnState::with_unverified_hash(
            OwnedTurnState {
                turn: 10,
                player1_position: Coordinates::new(2, 2),
                player2_position: Coordinates::new(0, 0),
                player1_score: 1.0,
                player2_score: 0.0,
                player1_mud_turns: 0,
                player2_mud_turns: 0,
                cheese: vec![Coordinates::new(4, 4)], // one cheese collected
                player1_last_move: Direction::Stay,
                player2_last_move: Direction::Stay,
            },
            0,
        ));

        let sim = state.to_sim();

        // total_cheese must reflect the original count, not remaining
        assert_eq!(
            sim.cheese.total_cheese() as usize,
            original_cheese_count,
            "to_sim() must preserve initial_cheese_count"
        );
        assert_eq!(sim.cheese_positions().len(), 1);

        // score=1 with 2 total cheese => threshold is 1.0, not over yet
        assert!(
            !sim.check_game_over(),
            "game should NOT be over: score 1.0 does not exceed half of 2 total cheese"
        );
    }

    #[test]
    fn distances_from_works() {
        let state = GameState::from_config(&test_config()).unwrap();
        let dists = state.distances_from(Some(Coordinates::new(0, 0)));
        assert_eq!(dists[&Coordinates::new(0, 0)], 0);
        assert_eq!(dists[&Coordinates::new(1, 0)], 1);
        assert_eq!(dists[&Coordinates::new(4, 4)], 8);
    }
}
