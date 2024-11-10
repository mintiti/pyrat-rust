use std::collections::HashMap;
use crate::{CheeseBoard, Coordinates, Direction, MoveTable};

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
        !self.is_in_mud() || self.mud_timer == 1  // Can collect on last mud turn
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
    /// Process a single game turn
    pub fn process_turn(&mut self, p1_move: Direction, p2_move: Direction) -> TurnResult {
        // Process player movements
        let (p1_moved, p2_moved) = self.process_moves(p1_move, p2_move);

        // Process cheese collection and scoring
        let cheese_collected = self.process_cheese_collection();

        // Update turn counter
        self.turn += 1;

        // Check game ending conditions
        let game_over = self.check_game_over();

        TurnResult {
            p1_moved,
            p2_moved,
            cheese_collected,
            game_over,
            p1_score: self.player1.score,
            p2_score: self.player2.score,
        }
    }

    #[inline]
    fn process_moves(&mut self, p1_move: Direction, p2_move: Direction) -> (bool, bool) {
        let (p1_moved, p1_new_pos) = self.compute_player_move(&self.player1, p1_move);
        let (p2_moved, p2_new_pos) = self.compute_player_move(&self.player2, p2_move);

        // Update player states with new positions and mud status
        if p1_moved {
            let mud_time = self.mud.get(&(self.player1.current_pos, p1_new_pos))
                .copied()
                .unwrap_or(0);
            if mud_time > 0 {
                self.player1.mud_timer = mud_time;
                self.player1.target_pos = p1_new_pos;
            } else {
                self.player1.current_pos = p1_new_pos;
                self.player1.target_pos = p1_new_pos;
            }
        } else if self.player1.is_in_mud() {
            self.player1.mud_timer -= 1;
            if self.player1.mud_timer == 0 {
                self.player1.current_pos = self.player1.target_pos;
            }
        } else {
            self.player1.misses += 1;
        }

        // Same for player 2
        if p2_moved {
            let mud_time = self.mud.get(&(self.player2.current_pos, p2_new_pos))
                .copied()
                .unwrap_or(0);
            if mud_time > 0 {
                self.player2.mud_timer = mud_time;
                self.player2.target_pos = p2_new_pos;
            } else {
                self.player2.current_pos = p2_new_pos;
                self.player2.target_pos = p2_new_pos;
            }
        } else if self.player2.is_in_mud() {
            self.player2.mud_timer -= 1;
            if self.player2.mud_timer == 0 {
                self.player2.current_pos = self.player2.target_pos;
            }
        } else {
            self.player2.misses += 1;
        }

        (p1_moved, p2_moved)
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
    fn process_cheese_collection(&mut self) -> bool {
        let mut cheese_collected = false;

        // Check for simultaneous cheese collection
        if self.player1.can_collect_cheese() &&
            self.player2.can_collect_cheese() &&
            self.player1.current_pos == self.player2.current_pos {
            if self.cheese.take_cheese(self.player1.current_pos) {
                self.player1.score += 0.5;
                self.player2.score += 0.5;
                cheese_collected = true;
            }
            return cheese_collected;
        }

        // Individual cheese collection
        if self.player1.can_collect_cheese() {
            if self.cheese.take_cheese(self.player1.current_pos) {
                self.player1.score += 1.0;
                cheese_collected = true;
            }
        }

        if self.player2.can_collect_cheese() {
            if self.cheese.take_cheese(self.player2.current_pos) {
                self.player2.score += 1.0;
                cheese_collected = true;
            }
        }

        cheese_collected
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
    pub cheese_collected: bool,
    pub game_over: bool,
    pub p1_score: f32,
    pub p2_score: f32,
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
            vec![Coordinates::new(1, 0)]
        );

        let mut mud = HashMap::new();
        mud.insert(
            (Coordinates::new(1, 1), Coordinates::new(1, 2)),
            2
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
                Coordinates::new(2, 2)
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
                Coordinates::new(2, 2)
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
                Coordinates::new(1, 2)
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
                &[((start_pos, target_pos), mud_timer)]
            );

            // Initial move into mud
            let result = game.process_turn(Direction::Up, Direction::Stay);
            assert!(result.p1_moved);
            assert_eq!(game.player1.mud_timer, mud_timer);
            assert_eq!(game.player1.current_pos, start_pos);
            assert_eq!(game.player1.target_pos, target_pos);

            // First mud turn
            let result = game.process_turn(Direction::Right, Direction::Stay);
            assert!(!result.p1_moved);
            assert_eq!(game.player1.mud_timer, 1);

            // Final mud turn - should complete movement
            let result = game.process_turn(Direction::Left, Direction::Stay);
            assert!(!result.p1_moved);
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
                Coordinates::new(2, 2)
            );

            game.cheese.place_cheese(Coordinates::new(1, 0));

            let result = game.process_turn(Direction::Right, Direction::Stay);

            assert!(result.cheese_collected);
            assert_eq!(game.player1.score, 1.0);
            assert_eq!(game.player2.score, 0.0);
            assert_eq!(game.cheese.remaining_cheese(), 0);
        }

        #[test]
        fn test_simultaneous_cheese_collection() {
            let mut game = create_test_game(
                Coordinates::new(0, 1),
                Coordinates::new(2, 1)
            );

            game.cheese.place_cheese(Coordinates::new(1, 1));

            let result = game.process_turn(Direction::Right, Direction::Left);

            assert!(result.cheese_collected);
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
                Coordinates::new(2, 2)
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
                Coordinates::new(2, 2)   // Player 2 starts top right
            );

            // Place 3 cheese pieces in a vertical line
            game.cheese.place_cheese(Coordinates::new(1, 0));  // First cheese
            game.cheese.place_cheese(Coordinates::new(1, 1));  // Second cheese
            game.cheese.place_cheese(Coordinates::new(1, 2));  // Third cheese

            // Move to and collect first cheese at (1,0)
            let result = game.process_turn(Direction::Right, Direction::Stay);
            assert!(result.cheese_collected);
            assert_eq!(game.player1.score, 1.0);
            assert_eq!(game.player1.current_pos, Coordinates::new(1, 0));

            // Move up and collect second cheese at (1,1)
            let result = game.process_turn(Direction::Up, Direction::Stay);
            assert!(result.cheese_collected);
            assert_eq!(game.player1.score, 2.0);  // Should now have 2 points
            assert!(result.game_over);  // Game should end as player1 has more than half the cheese
            assert_eq!(game.player1.current_pos, Coordinates::new(1, 1));
        }

        #[test]
        fn test_win_score_calculation() {
            let mut game = create_test_game(
                Coordinates::new(0, 0),
                Coordinates::new(2, 2)
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
                Coordinates::new(2, 1)
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