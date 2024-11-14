use std::collections::HashMap;
use crate::{CheeseBoard, Coordinates, Direction, MoveTable};
use crate::maze_generation::{CheeseConfig, CheeseGenerator, MazeConfig, MazeGenerator};

/// Stores the state of a player including their movement status
#[derive(Clone)]
struct PlayerState {
    current_pos: Coordinates,     // Position where player is visible/started move
    target_pos: Coordinates,      // Position player is moving to if in mud
    mud_timer: u8,               // Turns remaining in mud, 0 if not in mud
    score: f32,
    misses: u16,                 // Counter for missed moves
}

impl PlayerState {
    #[inline(always)]
    fn is_in_mud(&self) -> bool {
        self.mud_timer > 0
    }

    #[inline(always)]
    fn can_collect_cheese(&self) -> bool {
        !self.is_in_mud()  // Can collect on last mud turn
    }
}
/// Records what happened in a move for unmake purposes
#[derive(Clone)]
struct MoveUndo {
    // Player 1 state
    p1_pos: Coordinates,
    p1_target: Coordinates,
    p1_mud: u8,
    p1_score: f32,
    p1_misses: u16,

    // Player 2 state
    p2_pos: Coordinates,
    p2_target: Coordinates,
    p2_mud: u8,
    p2_score: f32,
    p2_misses: u16,

    // Cheese collected this move
    collected_cheese: Vec<Coordinates>,

    turn: u16,
}

impl MoveUndo {
    #[inline(always)]
    fn new(game: &GameState) -> Self {
        Self {
            p1_pos: game.player1.current_pos,
            p1_target: game.player1.target_pos,
            p1_mud: game.player1.mud_timer,
            p1_score: game.player1.score,
            p1_misses: game.player1.misses,

            p2_pos: game.player2.current_pos,
            p2_target: game.player2.target_pos,
            p2_mud: game.player2.mud_timer,
            p2_score: game.player2.score,
            p2_misses: game.player2.misses,

            collected_cheese: Vec::with_capacity(2),

            turn: game.turn,
        }
    }
}
#[derive(Clone)]
pub struct GameState {
    width: u8,
    height: u8,
    move_table: MoveTable,
    player1: PlayerState,
    player2: PlayerState,
    pub mud: HashMap<(Coordinates, Coordinates), u8>,
    pub cheese: CheeseBoard,
    turn: u16,
    max_turns: u16,
}


impl GameState {
    /// Default PyRat dimensions and cheese count
    pub const DEFAULT_WIDTH: u8 = 21;
    pub const DEFAULT_HEIGHT: u8 = 15;
    pub const DEFAULT_CHEESE_COUNT: u16 = 41;
    /// Creates a new game state with the given dimensions and walls
    ///
    /// # Arguments
    /// * `width` - Width of the game board
    /// * `height` - Height of the game board
    /// * `walls` - HashMap containing wall positions. Each key-value pair represents walls from a position
    /// * `max_turns` - Maximum number of turns before the game ends
    pub fn new(width: u8, height: u8, walls: HashMap<Coordinates, Vec<Coordinates>>, max_turns: u16) -> Self {
        // Create move table for efficient move validation
        let move_table = MoveTable::new(width, height, &walls);

        // Initialize players at opposite corners
        let player1 = PlayerState {
            current_pos: Coordinates::new(0, 0),  // Bottom left
            target_pos: Coordinates::new(0, 0),
            mud_timer: 0,
            score: 0.0,
            misses: 0,
        };

        let player2 = PlayerState {
            current_pos: Coordinates::new(width - 1, height - 1),  // Top right
            target_pos: Coordinates::new(width - 1, height - 1),
            mud_timer: 0,
            score: 0.0,
            misses: 0,
        };

        Self {
            width,
            height,
            move_table,
            player1,
            player2,
            mud: HashMap::new(),  // Start with no mud
            cheese: CheeseBoard::new(width, height),
            turn: 0,
            max_turns,
        }
    }

    /// Creates a new game state with customized player positions
    /// Useful for testing and specific scenarios
    pub fn new_with_positions(
        width: u8,
        height: u8,
        walls: HashMap<Coordinates, Vec<Coordinates>>,
        max_turns: u16,
        player1_pos: Coordinates,
        player2_pos: Coordinates,
    ) -> Self {
        let mut game = Self::new(width, height, walls, max_turns);
        game.player1.current_pos = player1_pos;
        game.player1.target_pos = player1_pos;
        game.player2.current_pos = player2_pos;
        game.player2.target_pos = player2_pos;
        game
    }

    /// Creates a new game state with the given configuration
    /// Useful when you want to specify everything at once
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_config(
        width: u8,
        height: u8,
        walls: HashMap<Coordinates, Vec<Coordinates>>,
        mud: HashMap<(Coordinates, Coordinates), u8>,
        cheese_positions: &[Coordinates],
        player1_pos: Coordinates,
        player2_pos: Coordinates,
        max_turns: u16,
    ) -> Self {
        let mut game = Self::new_with_positions(
            width,
            height,
            walls,
            max_turns,
            player1_pos,
            player2_pos,
        );

        // Add mud
        game.mud = mud;

        // Add cheese
        for &pos in cheese_positions {
            game.cheese.place_cheese(pos);
        }

        game
    }

    /// Creates a new randomized game state with the given configuration
    pub fn new_random(
        width: u8,
        height: u8,
        maze_config: MazeConfig,
        cheese_config: CheeseConfig,
    ) -> Self {
        // Validate dimensions match
        assert_eq!(width, maze_config.width, "Width mismatch in configurations");
        assert_eq!(height, maze_config.height, "Height mismatch in configurations");

        // Create default player positions (opposite corners)
        let player1_pos = Coordinates::new(0, 0);  // Bottom left
        let player2_pos = Coordinates::new(width - 1, height - 1);  // Top right

        // Generate maze layout
        let mut maze_gen = MazeGenerator::new(maze_config.clone());
        let (walls, mud) = maze_gen.generate();

        // Generate cheese positions
        let mut cheese_gen = CheeseGenerator::new(
            cheese_config,
            width,
            height,
            maze_config.seed, // Use same seed for reproducibility
        );
        let cheese_positions = cheese_gen.generate(player1_pos, player2_pos);

        // Create cheese board and place cheese
        let mut cheese_board = CheeseBoard::new(width, height);
        for pos in cheese_positions {
            cheese_board.place_cheese(pos);
        }

        // Create initial game state
        Self {
            width,
            height,
            move_table: MoveTable::new(width, height, &walls),
            player1: PlayerState {
                current_pos: player1_pos,
                target_pos: player1_pos,
                mud_timer: 0,
                score: 0.0,
                misses: 0,
            },
            player2: PlayerState {
                current_pos: player2_pos,
                target_pos: player2_pos,
                mud_timer: 0,
                score: 0.0,
                misses: 0,
            },
            mud,
            cheese: cheese_board,
            turn: 0,
            max_turns: 300, // Default from Python implementation
        }
    }

    /// Creates a new randomized symmetric game state with PyRat defaults
    pub fn new_symmetric(
        width: Option<u8>,
        height: Option<u8>,
        cheese_count: Option<u16>,
        seed: Option<u64>,
    ) -> Self {
        let width = width.unwrap_or(Self::DEFAULT_WIDTH);
        let height = height.unwrap_or(Self::DEFAULT_HEIGHT);
        let cheese_count = cheese_count.unwrap_or(Self::DEFAULT_CHEESE_COUNT);

        let maze_config = MazeConfig {
            width,
            height,
            target_density: 0.7,    // Common default
            connected: true,         // Always want connected mazes
            symmetry: true,          // Symmetric maze
            mud_density: 0.1,        // 10% mud probability
            mud_range: 3,            // Mud values 2-3
            seed,
        };

        let cheese_config = CheeseConfig {
            count: cheese_count,
            symmetry: true,          // Symmetric cheese placement
        };

        Self::new_random(width, height, maze_config, cheese_config)
    }

    /// Creates a new randomized asymmetric game state with PyRat defaults
    pub fn new_asymmetric(
        width: Option<u8>,
        height: Option<u8>,
        cheese_count: Option<u16>,
        seed: Option<u64>,
    ) -> Self {
        let width = width.unwrap_or(Self::DEFAULT_WIDTH);
        let height = height.unwrap_or(Self::DEFAULT_HEIGHT);
        let cheese_count = cheese_count.unwrap_or(Self::DEFAULT_CHEESE_COUNT);

        let maze_config = MazeConfig {
            width,
            height,
            target_density: 0.7,    // Common default
            connected: true,         // Always want connected mazes
            symmetry: false,         // Asymmetric maze
            mud_density: 0.1,        // 10% mud probability
            mud_range: 3,            // Mud values 2-3
            seed,
        };

        let cheese_config = CheeseConfig {
            count: cheese_count,
            symmetry: false,         // Asymmetric cheese placement
        };

        Self::new_random(width, height, maze_config, cheese_config)
    }
    /// Process a single game turn
    pub fn process_turn(&mut self, p1_move: Direction, p2_move: Direction) -> TurnResult {
        // Process player movements
        let (p1_moved, p2_moved) = self.process_moves(p1_move, p2_move);

        // Process cheese collection
        let collected_cheese = self.process_cheese_collection();

        // Update turn counter
        self.turn += 1;

        // Check game ending conditions
        let game_over = self.check_game_over();

        TurnResult {
            p1_moved,
            p2_moved,
            game_over,
            p1_score: self.player1.score,
            p2_score: self.player2.score,
            collected_cheese,
        }
    }

    /// Process a move and create undo information
    #[inline]
    pub fn make_move(&mut self, p1_move: Direction, p2_move: Direction) -> MoveUndo {
        // Save initial state
        let undo = MoveUndo {
            p1_pos: self.player1.current_pos,
            p1_target: self.player1.target_pos,
            p1_mud: self.player1.mud_timer,
            p1_score: self.player1.score,
            p1_misses: self.player1.misses,

            p2_pos: self.player2.current_pos,
            p2_target: self.player2.target_pos,
            p2_mud: self.player2.mud_timer,
            p2_score: self.player2.score,
            p2_misses: self.player2.misses,

            collected_cheese: Vec::with_capacity(2),
            turn: self.turn,
        };

        // Process turn and save collected cheese
        let result = self.process_turn(p1_move, p2_move);

        MoveUndo {
            collected_cheese: result.collected_cheese,
            ..undo
        }
    }

    /// Unmake a move using the saved undo information
    #[inline]
    pub fn unmake_move(&mut self, undo: MoveUndo) {
        // Restore any collected cheese
        for pos in undo.collected_cheese {
            self.cheese.restore_cheese(pos);
        }

        // Restore player states
        self.player1.current_pos = undo.p1_pos;
        self.player1.target_pos = undo.p1_target;
        self.player1.mud_timer = undo.p1_mud;
        self.player1.score = undo.p1_score;
        self.player1.misses = undo.p1_misses;

        self.player2.current_pos = undo.p2_pos;
        self.player2.target_pos = undo.p2_target;
        self.player2.mud_timer = undo.p2_mud;
        self.player2.score = undo.p2_score;
        self.player2.misses = undo.p2_misses;

        self.turn = undo.turn;
    }

    #[inline]
    fn process_moves(&mut self, p1_move: Direction, p2_move: Direction) -> (bool, bool) {
        // Store initial positions BEFORE any updates
        let p1_start_pos = self.player1.current_pos;
        let p2_start_pos = self.player2.current_pos;

        // Compute next positions first - this handles Stay and wall collisions
        let (p1_moved, p1_new_pos) = self.compute_player_move(&self.player1, p1_move);
        let (p2_moved, p2_new_pos) = self.compute_player_move(&self.player2, p2_move);

        // Update Player 1
        if self.player1.mud_timer > 0 {
            // In mud - decrement timer and possibly complete move
            self.player1.mud_timer -= 1;
            // Mud just ended - complete the move initiated previously
            if self.player1.mud_timer == 0 {
                self.player1.current_pos = self.player1.target_pos;
            }
        } else if p1_moved {
            // Not in mud - check new position
            let mud_time = self.mud.get(&(self.player1.current_pos, p1_new_pos))
                .copied()
                .unwrap_or(0);
            self.player1.mud_timer = mud_time;
            if mud_time > 1 {
                // Enter mud
                self.player1.target_pos = p1_new_pos;
            } else {
                // Move immediately (no mud or mud=1)
                self.player1.current_pos = p1_new_pos;
                self.player1.target_pos = p1_new_pos;
            }
        }

        // Update Player 2 (same logic)
        if self.player2.mud_timer > 0 {
            self.player2.mud_timer -= 1;
            if self.player2.mud_timer == 0 {
                self.player2.current_pos = self.player2.target_pos;
            }
        } else if p2_moved {
            let mud_time = self.mud.get(&(self.player2.current_pos, p2_new_pos))
                .copied()
                .unwrap_or(0);

            self.player2.mud_timer = mud_time;
            if mud_time > 1 {
                self.player2.target_pos = p2_new_pos;
            } else {
                self.player2.current_pos = p2_new_pos;
                self.player2.target_pos = p2_new_pos;
            }
        }

        // Process any remaining movement
        if self.player1.mud_timer == 0 && self.player1.current_pos != self.player1.target_pos {
            self.player1.current_pos = self.player1.target_pos;
        }
        if self.player2.mud_timer == 0 && self.player2.current_pos != self.player2.target_pos {
            self.player2.current_pos = self.player2.target_pos;
        }

        // Increment misses if position didn't change
        let p1_has_moved = self.player1.current_pos != p1_start_pos;
        let p2_has_moved = self.player2.current_pos != p2_start_pos;
        if !p1_has_moved {
            self.player1.misses += 1;
        }
        if !p2_has_moved {
            self.player2.misses += 1;
        }

        (p1_has_moved, p2_has_moved)
    }

    #[inline]
    fn compute_player_move(&self, player: &PlayerState, move_dir: Direction) -> (bool, Coordinates) {
        if player.is_in_mud() {
            return (false, player.current_pos);
        }

        if move_dir == Direction::Stay {
            return (false, player.current_pos);
        }

        if !self.move_table.is_move_valid(player.current_pos, move_dir) {
            return (false, player.current_pos);
        }

        // Move must be valid at this point
        (true, move_dir.apply_to(player.current_pos))
    }
    #[inline]
    fn compute_destination(&self, pos: Coordinates, move_dir: Direction) -> Coordinates {
        if move_dir == Direction::Stay || !self.move_table.is_move_valid(pos, move_dir) {
            pos
        } else {
            move_dir.apply_to(pos)
        }
    }

    #[inline]
    fn process_cheese_collection(&mut self) -> Vec<Coordinates> {
        let mut collected = Vec::with_capacity(2);

        // Check for simultaneous collection first
        if self.player1.can_collect_cheese() &&
            self.player2.can_collect_cheese() &&
            self.player1.current_pos == self.player2.current_pos {
            if self.cheese.take_cheese(self.player1.current_pos) {
                self.player1.score += 0.5;
                self.player2.score += 0.5;
                collected.push(self.player1.current_pos);
            }
            return collected;
        }

        // Individual collections
        if self.player1.can_collect_cheese() {
            if self.cheese.take_cheese(self.player1.current_pos) {
                self.player1.score += 1.0;
                collected.push(self.player1.current_pos);
            }
        }

        if self.player2.can_collect_cheese() {
            if self.cheese.take_cheese(self.player2.current_pos) {
                self.player2.score += 1.0;
                collected.push(self.player2.current_pos);
            }
        }

        collected
    }

    fn check_game_over(&self) -> bool {
        // Check win conditions:
        // 1. Player scored more than half the total cheese (not remaining)
        let total_cheese = self.cheese.total_cheese() as f32;
        let half_cheese = total_cheese / 2.0;

        if self.player1.score > half_cheese || self.player2.score > half_cheese {
            return true;
        }

        // 2. All cheese collected
        if self.cheese.remaining_cheese() == 0 && total_cheese > 0.0 {
            return true;
        }

        // 3. Maximum turns reached
        self.turn >= self.max_turns
    }
}

pub struct TurnResult {
    pub p1_moved: bool,
    pub p2_moved: bool,
    pub game_over: bool,
    pub p1_score: f32,
    pub p2_score: f32,
    pub collected_cheese: Vec<Coordinates>,  // Collected this turn
}

// Implement Debug for nicer error messages and testing
impl std::fmt::Debug for GameState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GameState {{ turn: {}, p1_score: {}, p2_score: {}, cheese_remaining: {} }}",
               self.turn, self.player1.score, self.player2.score, self.cheese.remaining_cheese())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_new_game_state() {
        let width = 10;
        let height = 10;
        let game = GameState::new(width, height, HashMap::new(), 300);

        // Check dimensions
        assert_eq!(game.width, width);
        assert_eq!(game.height, height);

        // Check player positions
        assert_eq!(game.player1.current_pos, Coordinates::new(0, 0));
        assert_eq!(game.player2.current_pos, Coordinates::new(width - 1, height - 1));

        // Check initial state
        assert_eq!(game.turn, 0);
        assert_eq!(game.max_turns, 300);
        assert_eq!(game.player1.score, 0.0);
        assert_eq!(game.player2.score, 0.0);
        assert_eq!(game.cheese.total_cheese(), 0);
        assert!(game.mud.is_empty());
    }

    #[test]
    fn test_new_with_positions() {
        let p1_pos = Coordinates::new(1, 1);
        let p2_pos = Coordinates::new(2, 2);
        let game = GameState::new_with_positions(
            3, 3,
            HashMap::new(),
            300,
            p1_pos,
            p2_pos,
        );

        assert_eq!(game.player1.current_pos, p1_pos);
        assert_eq!(game.player1.target_pos, p1_pos);
        assert_eq!(game.player2.current_pos, p2_pos);
        assert_eq!(game.player2.target_pos, p2_pos);
    }

    #[test]
    fn test_new_with_config() {
        let width = 3;
        let height = 3;
        let mut walls = HashMap::new();
        walls.insert(
            Coordinates::new(0, 0),
            vec![Coordinates::new(1, 0)],
        );

        let mut mud = HashMap::new();
        mud.insert(
            (Coordinates::new(1, 1), Coordinates::new(1, 2)),
            2,
        );

        let cheese_positions = vec![
            Coordinates::new(1, 1),
            Coordinates::new(2, 2),
        ];

        let game = GameState::new_with_config(
            width,
            height,
            walls,
            mud,
            &cheese_positions,
            Coordinates::new(0, 0),
            Coordinates::new(2, 2),
            300,
        );

        // Check everything was configured correctly
        assert_eq!(game.width, width);
        assert_eq!(game.height, height);
        assert_eq!(game.cheese.total_cheese(), 2);
        assert!(game.mud.contains_key(&(
            Coordinates::new(1, 1),
            Coordinates::new(1, 2)
        )));
        assert_eq!(game.mud.len(), 1);
    }
    #[test]
    fn test_new_random_basic() {
        let width = 8;
        let height = 8;
        let maze_config = MazeConfig {
            width,
            height,
            target_density: 0.7,
            connected: true,
            symmetry: false,
            mud_density: 0.1,
            mud_range: 3,
            seed: Some(42),
        };

        let cheese_config = CheeseConfig {
            count: 10,
            symmetry: false,
        };

        let game = GameState::new_random(width, height, maze_config, cheese_config);

        // Check basic properties
        assert_eq!(game.width, width);
        assert_eq!(game.height, height);
        assert_eq!(game.cheese.total_cheese(), 10);
        assert_eq!(game.player1.current_pos, Coordinates::new(0, 0));
        assert_eq!(game.player2.current_pos, Coordinates::new(width - 1, height - 1));
    }

    #[test]
    fn test_new_symmetric() {
        let game = GameState::new_symmetric(Some(11), Some(11), Some(15), Some(42));

        // Verify symmetry
        let center_x = game.width / 2;
        let center_y = game.height / 2;

        // Check if cheese placement is symmetric
        let cheese_positions = game.cheese.get_all_cheese_positions();
        for pos in &cheese_positions {
            let symmetric_pos = Coordinates::new(
                game.width - 1 - pos.x,
                game.height - 1 - pos.y,
            );
            if *pos != symmetric_pos { // Ignore center piece if it exists
                assert!(
                    cheese_positions.contains(&symmetric_pos),
                    "Missing symmetric cheese piece for {:?}", pos
                );
            }
        }
    }

    #[test]
    fn test_new_asymmetric() {
        let game = GameState::new_asymmetric(Some(8), Some(8), Some(10), Some(42));

        // Basic structure tests
        assert_eq!(game.width, 8);
        assert_eq!(game.height, 8);
        assert_eq!(game.cheese.total_cheese(), 10);
    }

    #[test]
    fn test_reproducibility() {
        let seed = Some(42);
        let game1 = GameState::new_symmetric(Some(8), Some(8), Some(10), seed);
        let game2 = GameState::new_symmetric(Some(8), Some(8), Some(10), seed);

        // Games should be identical with same seed
        assert_eq!(
            game1.cheese.get_all_cheese_positions(),
            game2.cheese.get_all_cheese_positions()
        );
    }

    // Helper function to create a simple 3x3 game state for testing
    fn create_test_game(player1_pos: Coordinates, player2_pos: Coordinates) -> GameState {
        let width = 3;
        let height = 3;
        let walls = HashMap::new();

        GameState {
            width,
            height,
            move_table: MoveTable::new(width, height, &walls),
            player1: PlayerState {
                current_pos: player1_pos,
                target_pos: player1_pos,
                mud_timer: 0,
                score: 0.0,
                misses: 0,
            },
            player2: PlayerState {
                current_pos: player2_pos,
                target_pos: player2_pos,
                mud_timer: 0,
                score: 0.0,
                misses: 0,
            },
            mud: HashMap::new(),
            cheese: CheeseBoard::new(width, height),
            turn: 0,
            max_turns: 300,
        }
    }

    // Helper to create a game state with mud
    fn create_game_with_mud(
        player1_pos: Coordinates,
        player2_pos: Coordinates,
        mud_positions: &[((Coordinates, Coordinates), u8)],
    ) -> GameState {
        let mut game = create_test_game(player1_pos, player2_pos);
        for &(positions, timer) in mud_positions {
            game.mud.insert(positions, timer);
        }
        game
    }

    mod basic_movement {
        use super::*;

        #[test]
        fn test_basic_movement() {
            let mut game = create_test_game(
                Coordinates::new(0, 0),
                Coordinates::new(2, 2),
            );

            let result = game.process_turn(Direction::Right, Direction::Left);

            assert!(result.p1_moved);
            assert!(result.p2_moved);
            assert_eq!(game.player1.current_pos, Coordinates::new(1, 0));
            assert_eq!(game.player2.current_pos, Coordinates::new(1, 2));
        }

        #[test]
        fn test_boundary_movement() {
            let mut game = create_test_game(
                Coordinates::new(0, 0),
                Coordinates::new(2, 2),
            );

            let result = game.process_turn(Direction::Left, Direction::Right);

            assert!(!result.p1_moved);
            assert!(!result.p2_moved);
            assert_eq!(game.player1.misses, 1);
            assert_eq!(game.player2.misses, 1);
        }

        #[test]
        fn test_stay_command() {
            let mut game = create_test_game(
                Coordinates::new(1, 1),
                Coordinates::new(1, 2),
            );

            let result = game.process_turn(Direction::Stay, Direction::Stay);

            assert!(!result.p1_moved);
            assert!(!result.p2_moved);
            assert_eq!(game.player1.current_pos, Coordinates::new(1, 1));
            assert_eq!(game.player2.current_pos, Coordinates::new(1, 2));
        }
    }

    mod mud_mechanics {
        use super::*;

        #[test]
        fn test_mud_movement() {
            let mud_timer = 2;
            let start_pos = Coordinates::new(1, 1);
            let target_pos = Coordinates::new(1, 2);

            let mut game = create_game_with_mud(
                start_pos,
                Coordinates::new(0, 0),
                &[((start_pos, target_pos), mud_timer)],
            );

            // Initial move into mud
            let result = game.process_turn(Direction::Up, Direction::Stay);
            assert!(!result.p1_moved);
            assert_eq!(game.player1.current_pos, start_pos);
            assert_eq!(game.player1.target_pos, target_pos);

            // First mud turn
            let result = game.process_turn(Direction::Right, Direction::Stay);
            assert!(!result.p1_moved);
            assert_eq!(game.player1.mud_timer, 1);
            assert_eq!(game.player1.current_pos, start_pos);
            assert_eq!(game.player1.target_pos, target_pos);


            // Final mud turn - should complete movement
            let result = game.process_turn(Direction::Left, Direction::Stay);
            assert!(result.p1_moved);
            assert_eq!(game.player1.mud_timer, 0);
            assert_eq!(game.player1.current_pos, target_pos);
        }
    }

    mod cheese_collection {
        use super::*;

        #[test]
        fn test_basic_cheese_collection() {
            let mut game = create_test_game(
                Coordinates::new(0, 0),
                Coordinates::new(2, 2),
            );

            game.cheese.place_cheese(Coordinates::new(1, 0));

            let result = game.process_turn(Direction::Right, Direction::Stay);

            assert_eq!(result.collected_cheese.len(),1);
            assert!(result.collected_cheese.contains(&Coordinates::new(1, 0)));
            assert_eq!(game.player1.score, 1.0);
            assert_eq!(game.player2.score, 0.0);
            assert_eq!(game.cheese.remaining_cheese(), 0);
        }

        #[test]
        fn test_simultaneous_cheese_collection() {
            let mut game = create_test_game(
                Coordinates::new(0, 1),
                Coordinates::new(2, 1),
            );

            game.cheese.place_cheese(Coordinates::new(1, 1));

            let result = game.process_turn(Direction::Right, Direction::Left);

            assert_eq!(result.collected_cheese.len(), 1);
            assert!(result.collected_cheese.contains(&Coordinates::new(1, 1)));
            assert_eq!(game.player1.score, 0.5);
            assert_eq!(game.player2.score, 0.5);
            assert_eq!(game.cheese.remaining_cheese(), 0);
        }
    }

    mod game_ending {
        use super::*;

        #[test]
        fn test_max_turns() {
            let mut game = create_test_game(
                Coordinates::new(0, 0),
                Coordinates::new(2, 2),
            );
            game.max_turns = 2;

            // First turn
            let result = game.process_turn(Direction::Stay, Direction::Stay);
            assert!(!result.game_over);
            assert_eq!(game.turn, 1);

            // Second turn (should end game)
            let result = game.process_turn(Direction::Stay, Direction::Stay);
            assert!(result.game_over);
            assert_eq!(game.turn, 2);
        }

        #[test]
        fn test_win_by_score() {
            let mut game = create_test_game(
                Coordinates::new(0, 0),  // Player 1 starts bottom left
                Coordinates::new(2, 2),   // Player 2 starts top right
            );

            // Place 3 cheese pieces in a vertical line
            game.cheese.place_cheese(Coordinates::new(1, 0));  // First cheese
            game.cheese.place_cheese(Coordinates::new(1, 1));  // Second cheese
            game.cheese.place_cheese(Coordinates::new(1, 2));  // Third cheese

            // Move to and collect first cheese at (1,0)
            let result = game.process_turn(Direction::Right, Direction::Stay);
            assert_eq!(result.collected_cheese.len(),1);
            assert!(result.collected_cheese.contains(&Coordinates::new(1, 0)));
            assert_eq!(game.player1.score, 1.0);
            assert_eq!(game.player1.current_pos, Coordinates::new(1, 0));

            // Move up and collect second cheese at (1,1)
            let result = game.process_turn(Direction::Up, Direction::Stay);
            assert_eq!(result.collected_cheese.len(), 1);
            assert!(result.collected_cheese.contains(&Coordinates::new(1, 1)));
            assert_eq!(game.player1.score, 2.0);  // Should now have 2 points
            assert!(result.game_over);  // Game should end as player1 has more than half the cheese
            assert_eq!(game.player1.current_pos, Coordinates::new(1, 1));
        }

        #[test]
        fn test_win_score_calculation() {
            let mut game = create_test_game(
                Coordinates::new(0, 0),
                Coordinates::new(2, 2),
            );

            // Place 5 cheese pieces - need 3 to win
            game.cheese.place_cheese(Coordinates::new(1, 0));
            game.cheese.place_cheese(Coordinates::new(1, 1));
            game.cheese.place_cheese(Coordinates::new(1, 2));
            game.cheese.place_cheese(Coordinates::new(2, 0));
            game.cheese.place_cheese(Coordinates::new(2, 1));

            // Collect first cheese
            let result = game.process_turn(Direction::Right, Direction::Stay);
            assert_eq!(game.player1.score, 1.0);
            assert!(!result.game_over);

            // Collect second cheese
            let result = game.process_turn(Direction::Up, Direction::Stay);
            assert_eq!(game.player1.score, 2.0);
            assert!(!result.game_over);  // 2 < 5/2, so game continues

            // Collect third cheese
            let result = game.process_turn(Direction::Up, Direction::Stay);
            assert_eq!(game.player1.score, 3.0);
            assert!(result.game_over);  // 3 > 5/2, so game ends
        }

        #[test]
        fn test_all_cheese_collected() {
            let mut game = create_test_game(
                Coordinates::new(0, 1),
                Coordinates::new(2, 1),
            );

            // Place single cheese in middle
            game.cheese.place_cheese(Coordinates::new(1, 1));

            // Both players move to cheese for simultaneous collection
            let result = game.process_turn(Direction::Right, Direction::Left);

            assert!(result.game_over);
            assert_eq!(game.player1.score, 0.5);
            assert_eq!(game.player2.score, 0.5);
            assert_eq!(game.cheese.remaining_cheese(), 0);
        }
    }
}
#[cfg(test)]
mod make_unmake_tests {
    use super::*;

    /// Helper to create a game with walls in specific positions
    fn create_game_with_walls() -> GameState {
        let mut walls = HashMap::new();
        // Create a vertical wall in the middle
        walls.insert(
            Coordinates::new(1, 0),
            vec![Coordinates::new(1, 1)],
        );
        walls.insert(
            Coordinates::new(1, 1),
            vec![Coordinates::new(1, 0)],
        );
        GameState::new(3, 3, walls, 300)
    }

    /// Helper to create a game with mud in specific positions
    fn create_game_with_mud() -> GameState {
        let mut game = GameState::new(3, 3, HashMap::new(), 300);
        game.mud.insert(
            (Coordinates::new(0, 0), Coordinates::new(0, 1)),
            2,  // 2 turns of mud
        );
        game
    }

    #[test]
    fn test_make_unmake_wall_collision() {
        let mut game = create_test_game_with_walls();
        let initial_state = game.clone();

        // Player 1 tries to move into wall
        // Starting at (0,0), trying to move right into wall at x=1
        let undo = game.make_move(Direction::Right, Direction::Stay);

        // Position should not change due to wall
        assert_eq!(game.player1.current_pos, initial_state.player1.current_pos,
                   "Player shouldn't move through wall");
        assert_eq!(game.player1.misses, initial_state.player1.misses + 1,
                   "Miss count should increment on wall collision");

        // Unmake move
        game.unmake_move(undo);

        // Everything should be exactly as it was
        assert_eq!(game.player1.current_pos, initial_state.player1.current_pos,
                   "Position not restored after wall unmake");
        assert_eq!(game.player1.misses, initial_state.player1.misses,
                   "Misses not restored after wall unmake");
    }

    #[test]
    fn test_make_unmake_mud_movement() {
        let mut game = create_game_with_mud();
        let initial_state = game.clone();

        // Move into mud
        let undo1 = game.make_move(Direction::Up, Direction::Stay);

        // Check mud state
        assert_eq!(game.player1.mud_timer, 2);
        assert_eq!(game.player1.current_pos, initial_state.player1.current_pos);
        assert_eq!(game.player1.target_pos, Coordinates::new(0, 1));

        // Try to move while in mud (should be ignored)
        let undo2 = game.make_move(Direction::Right, Direction::Stay);

        // Check still in mud, timer decreased
        assert_eq!(game.player1.mud_timer, 1);
        assert_eq!(game.player1.current_pos, initial_state.player1.current_pos);
        assert_eq!(game.player1.target_pos, Coordinates::new(0, 1));

        // Unmake moves in reverse order
        game.unmake_move(undo2);
        game.unmake_move(undo1);

        // Verify back to initial state
        assert_eq!(game.player1.mud_timer, initial_state.player1.mud_timer);
        assert_eq!(game.player1.current_pos, initial_state.player1.current_pos);
        assert_eq!(game.player1.target_pos, initial_state.player1.target_pos);
    }


    #[test]
    fn test_make_unmake_cheese_collection_in_mud() {
        let mut game = create_test_game_with_mud();

        // Place cheese at mud exit
        let cheese_pos = Coordinates::new(0, 1);
        game.cheese.place_cheese(cheese_pos);
        let initial_total = game.cheese.total_cheese();
        let initial_remaining = game.cheese.remaining_cheese();

        // Print initial state
        println!("Initial state - total: {}, remaining: {}", initial_total, initial_remaining);

        // Enter mud
        let undo1 = game.make_move(Direction::Up, Direction::Stay);
        println!("After mud enter - remaining: {}", game.cheese.remaining_cheese());
        assert_eq!(game.cheese.remaining_cheese(), initial_remaining,
                   "Cheese shouldn't be collected while entering mud");

        // Wait in mud
        let undo2 = game.make_move(Direction::Stay, Direction::Stay);
        println!("During mud - remaining: {}", game.cheese.remaining_cheese());
        assert_eq!(game.cheese.remaining_cheese(), initial_remaining,
                   "Cheese shouldn't be collected while in mud");

        // Exit mud and collect
        let undo3 = game.make_move(Direction::Stay, Direction::Stay);
        println!("After collection - remaining: {}", game.cheese.remaining_cheese());
        assert_eq!(game.cheese.remaining_cheese(), initial_remaining - 1,
                   "Cheese should be collected when exiting mud");

        // Unmake moves in reverse order
        game.unmake_move(undo3);
        println!("After unmake 3 - remaining: {}", game.cheese.remaining_cheese());
        game.unmake_move(undo2);
        println!("After unmake 2 - remaining: {}", game.cheese.remaining_cheese());
        game.unmake_move(undo1);
        println!("After unmake 1 - remaining: {}", game.cheese.remaining_cheese());

        assert_eq!(game.cheese.remaining_cheese(), initial_remaining,
                   "Remaining cheese not restored after unmake");
        assert_eq!(game.cheese.total_cheese(), initial_total,
                   "Total cheese changed during make/unmake");
    }
    #[test]
    fn test_make_unmake_simultaneous_mud_movement() {
        let mut game = GameState::new(3, 3, HashMap::new(), 300);

        // Add mud for both players
        game.mud.insert((Coordinates::new(0, 0), Coordinates::new(0, 1)), 2);
        game.mud.insert((Coordinates::new(2, 2), Coordinates::new(2, 1)), 3);

        let initial_state = game.clone();

        // Both players move into mud
        let undo1 = game.make_move(Direction::Up, Direction::Down);

        // Verify different mud timers
        assert_eq!(game.player1.mud_timer, 2);
        assert_eq!(game.player2.mud_timer, 3);

        // One more move
        let undo2 = game.make_move(Direction::Right, Direction::Left);

        // Verify timers decreased independently
        assert_eq!(game.player1.mud_timer, 1);
        assert_eq!(game.player2.mud_timer, 2);

        // Unmake moves
        game.unmake_move(undo2);
        game.unmake_move(undo1);

        // Verify restored to initial state
        assert_eq!(game.player1.mud_timer, initial_state.player1.mud_timer);
        assert_eq!(game.player2.mud_timer, initial_state.player2.mud_timer);
        assert_eq!(game.player1.current_pos, initial_state.player1.current_pos);
        assert_eq!(game.player2.current_pos, initial_state.player2.current_pos);
    }

    #[test]
    fn test_make_unmake_boundary_collision() {
        let mut game = GameState::new(3, 3, HashMap::new(), 300);
        let initial_state = game.clone();

        // Try to move outside board boundaries
        let moves = [
            (Direction::Left, "left boundary"),   // Try move left from (0,0)
            (Direction::Down, "bottom boundary"), // Try move down from (0,0)
        ];

        for (direction, description) in moves.iter() {
            let undo = game.make_move(*direction, Direction::Stay);

            // Verify collision behavior
            assert_eq!(
                game.player1.current_pos,
                initial_state.player1.current_pos,
                "Position changed on {} collision", description
            );
            assert_eq!(
                game.player1.misses,
                initial_state.player1.misses + 1,
                "Misses not incremented on {} collision", description
            );

            // Unmake move
            game.unmake_move(undo);

            // Verify restoration
            assert_eq!(
                game.player1.current_pos,
                initial_state.player1.current_pos,
                "Position not restored after {} collision", description
            );
            assert_eq!(
                game.player1.misses,
                initial_state.player1.misses,
                "Misses not restored after {} collision", description
            );
        }
    }

    #[test]
    fn test_make_unmake_complex_sequence() {
        let mut game = GameState::new(3, 3, HashMap::new(), 300);

        // Place cheese pieces
        game.cheese.place_cheese(Coordinates::new(1, 2));
        game.cheese.place_cheese(Coordinates::new(2, 2));

        // Add mud
        game.mud.insert((Coordinates::new(1, 1), Coordinates::new(1, 2)), 2);

        let initial_state = game.clone();
        let mut undo_stack = Vec::new();

        // Record initial positions
        let initial_p1_pos = game.player1.current_pos;  // (0,0)
        let initial_p2_pos = game.player2.current_pos;  // (2,2)

        // Better sequence of moves that stays within the grid
        let moves = [
            (Direction::Right, Direction::Left),  // P1: 0,0 -> 1,0  | P2: 2,2 -> 1,2
            (Direction::Up, Direction::Down),     // P1: 1,0 -> 1,1  | P2: 1,2 -> 1,1
            (Direction::Stay, Direction::Left),   // P1: in mud      | P2: 1,1 -> 0,1
            (Direction::Stay, Direction::Down),   // P1: in mud      | P2: 0,1 -> 0,0
            (Direction::Right, Direction::Right), // P1: 1,2         | P2: 0,0 -> 1,0
        ];

        println!("Initial positions - P1: {:?}, P2: {:?}", game.player1.current_pos, game.player2.current_pos);

        // Execute moves
        for (i, (p1_move, p2_move)) in moves.iter().enumerate() {
            let undo = game.make_move(*p1_move, *p2_move);
            println!("After move {} - P1: {:?}, P2: {:?}", i+1, game.player1.current_pos, game.player2.current_pos);
            undo_stack.push(undo);
        }

        // Verify positions actually changed
        println!("Final positions - P1: {:?}, P2: {:?}", game.player1.current_pos, game.player2.current_pos);
        assert_ne!(game.player1.current_pos, initial_p1_pos,
                   "Player 1 position should change after moves");
        assert_ne!(game.player2.current_pos, initial_p2_pos,
                   "Player 2 position should change after moves");

        // Unmake moves in reverse order
        while let Some(undo) = undo_stack.pop() {
            game.unmake_move(undo);
        }

        // Verify complete restoration
        assert_eq!(game.player1.current_pos, initial_state.player1.current_pos);
        assert_eq!(game.player1.target_pos, initial_state.player1.target_pos);
        assert_eq!(game.player1.mud_timer, initial_state.player1.mud_timer);
        assert_eq!(game.player1.score, initial_state.player1.score);
        assert_eq!(game.player1.misses, initial_state.player1.misses);
    }
    fn test_make_unmake_basic_move() {
        let mut game = GameState::new(3, 3, HashMap::new(), 300);

        // Make initial state different from default
        game.player1.score = 1.0;
        game.player2.misses = 2;
        let initial_state = game.clone();

        // Make a move
        let undo = game.make_move(Direction::Right, Direction::Left);

        // State should be different
        assert_ne!(game.player1.current_pos, initial_state.player1.current_pos);

        // Unmake the move
        game.unmake_move(undo);

        // State should be restored exactly
        assert_eq!(game.player1.current_pos, initial_state.player1.current_pos);
        assert_eq!(game.player1.score, initial_state.player1.score);
        assert_eq!(game.player2.misses, initial_state.player2.misses);
    }

    fn test_make_unmake_cheese_collection() {
        let mut game = GameState::new(3, 3, HashMap::new(), 300);

        // Place a cheese piece
        let cheese_pos = Coordinates::new(1, 0);
        game.cheese.place_cheese(cheese_pos);
        let initial_cheese_count = game.cheese.total_cheese();
        let initial_remaining = game.cheese.remaining_cheese();

        // Make a move that collects cheese
        let undo = game.make_move(Direction::Right, Direction::Stay);

        // Verify cheese was collected
        assert_eq!(game.cheese.remaining_cheese(), initial_remaining - 1);
        assert_eq!(game.cheese.total_cheese(), initial_cheese_count); // Should be unchanged
        assert_eq!(game.player1.score, 1.0);

        // Unmake the move
        game.unmake_move(undo);

        // Verify cheese was restored
        assert_eq!(game.cheese.remaining_cheese(), initial_remaining);
        assert_eq!(game.cheese.total_cheese(), initial_cheese_count); // Should still be unchanged
        assert!(game.cheese.has_cheese(cheese_pos));
        assert_eq!(game.player1.score, 0.0);
    }

    #[test]
    fn test_make_unmake_simultaneous_collection() {
        let mut game = GameState::new(3, 3, HashMap::new(), 300);

        // Place cheese where both players can reach it
        let cheese_pos = Coordinates::new(1, 1);
        game.cheese.place_cheese(cheese_pos);

        // Position players adjacent to cheese
        game.player1.current_pos = Coordinates::new(0, 1);
        game.player1.target_pos = Coordinates::new(0,1);
        game.player2.current_pos = Coordinates::new(2, 1);
        game.player2.target_pos = Coordinates::new(2,1);

        // Make move where both players collect cheese
        let undo = game.make_move(Direction::Right, Direction::Left);

        // Verify simultaneous collection
        assert_eq!(game.player1.score, 0.5);
        assert_eq!(game.player2.score, 0.5);
        assert!(!game.cheese.has_cheese(cheese_pos));

        // Unmake move
        game.unmake_move(undo);

        // Verify restoration
        assert_eq!(game.player1.score, 0.0);
        assert_eq!(game.player2.score, 0.0);
        assert!(game.cheese.has_cheese(cheese_pos));
    }
    /// Helper to create a test game with specific wall configuration
    fn create_test_game_with_walls() -> GameState {
        let mut walls = HashMap::new();
        walls.insert(Coordinates::new(1, 0), vec![Coordinates::new(0, 0)]);
        walls.insert(Coordinates::new(0, 0), vec![Coordinates::new(1, 0)]);
        GameState::new(3, 3, walls, 300)
    }

    /// Helper to create a test game with mud
    fn create_test_game_with_mud() -> GameState {
        let mut game = GameState::new(3, 3, HashMap::new(), 300);
        game.mud.insert(
            (Coordinates::new(0, 0), Coordinates::new(0, 1)),
            2  // 2 turns of mud
        );
        game
    }
}