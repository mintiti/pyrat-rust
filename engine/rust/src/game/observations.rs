use crate::{Coordinates, Direction, GameState};
use ndarray::{Array2, Array3};
use numpy::{PyArray2, PyArray3, PyArrayMethods};
use pyo3::sync::GILOnceCell;
use pyo3::{Py, Python};
use std::convert::TryFrom;
use std::sync::Arc;

/// Represents the movement constraints (walls and mud) for each position and direction
#[derive(Clone)]
pub struct MovementConstraints {
    /// Shape: [width, height, 4] where last dimension represents directions (UP, RIGHT, DOWN, LEFT)
    /// Values: -1 for walls/invalid moves, 0 for valid moves, >0 for mud turns
    pub matrix: Array3<i8>,
}

impl MovementConstraints {
    pub fn new(game: &GameState) -> Self {
        let width = game.width() as usize;
        let height = game.height() as usize;
        let mut matrix = Array3::zeros((width, height, 4));

        // Fill movement constraints
        for y in 0..height {
            for x in 0..width {
                let x = u8::try_from(x).expect("Width should fit in u8");
                let y = u8::try_from(y).expect("Height should fit in u8");
                let pos = Coordinates::new(x, y);

                // Check each direction
                for dir in [
                    Direction::Up,
                    Direction::Right,
                    Direction::Down,
                    Direction::Left,
                ] {
                    let dir_idx = dir as usize;

                    // Check boundary conditions first
                    let is_invalid = match dir {
                        Direction::Left => x == 0,
                        Direction::Right => usize::from(x) >= width - 1,
                        Direction::Down => y == 0,
                        Direction::Up => usize::from(y) >= height - 1,
                        Direction::Stay => false,
                    };

                    if is_invalid {
                        matrix[[usize::from(x), usize::from(y), dir_idx]] = -1;
                        continue;
                    }

                    // Then check if move is valid according to move table
                    if !game.move_table.is_move_valid(pos, dir) {
                        matrix[[usize::from(x), usize::from(y), dir_idx]] = -1;
                        continue;
                    }

                    // Check mud (now using bidirectional MudMap)
                    let target = dir.apply_to(pos);
                    if let Some(mud_turns) = game.mud.get(pos, target) {
                        matrix[[usize::from(x), usize::from(y), dir_idx]] = mud_turns as i8;
                    }
                }
            }
        }

        Self { matrix }
    }
}

/// Lazily materialized Python movement matrix shared by one maze topology.
#[derive(Clone, Default)]
pub(crate) struct MovementMatrixCache {
    matrix: Arc<GILOnceCell<Py<PyArray3<i8>>>>,
}

impl MovementMatrixCache {
    pub(crate) fn get(&self, py: Python<'_>, game: &GameState) -> Py<PyArray3<i8>> {
        self.matrix
            .get_or_init(py, || {
                let matrix = MovementConstraints::new(game).matrix;
                let array = PyArray3::from_owned_array(py, matrix);
                array.readwrite().make_nonwriteable();
                array.unbind()
            })
            .clone_ref(py)
    }
}

/// Manages game observations and their efficient updates
#[derive(Clone)]
pub struct ObservationHandler {
    movement_matrix: MovementMatrixCache,
    pub(crate) cheese_matrix: Array2<u8>,
}

impl ObservationHandler {
    pub fn new(game: &GameState) -> Self {
        Self::with_movement_cache(game, MovementMatrixCache::default())
    }

    pub(crate) fn with_movement_cache(
        game: &GameState,
        movement_matrix: MovementMatrixCache,
    ) -> Self {
        let mut handler = Self {
            movement_matrix,
            cheese_matrix: Array2::zeros((game.width() as usize, game.height() as usize)),
        };

        // Initialize cheese matrix
        for pos in game.cheese_positions() {
            handler.cheese_matrix[[pos.x as usize, pos.y as usize]] = 1;
        }

        handler
    }

    /// Update cheese positions based on collected cheese from turn result
    #[inline]
    pub fn update_collected_cheese(&mut self, collected: &[Coordinates]) {
        // Just clear the collected positions
        for pos in collected {
            self.cheese_matrix[[pos.x as usize, pos.y as usize]] = 0;
        }
    }

    /// Force full refresh of cheese matrix
    /// Only needed for unmake_move or reset operations
    #[inline]
    pub fn refresh_cheese(&mut self, game: &GameState) {
        self.cheese_matrix.fill(0);
        for pos in game.cheese_positions() {
            self.cheese_matrix[[pos.x as usize, pos.y as usize]] = 1;
        }
    }

    /// Get current observation for a player
    pub fn get_observation(
        &self,
        py: Python<'_>,
        game: &GameState,
        is_player_one: bool,
    ) -> GameObservation {
        let cheese_matrix = PyArray2::from_array(py, &self.cheese_matrix).unbind();
        let movement_matrix = self.movement_matrix.get(py, game);
        Self::build_observation(game, is_player_one, cheese_matrix, movement_matrix)
    }

    /// Get both player-relative observations over one pair of matrix snapshots.
    pub fn get_observations(&self, py: Python<'_>, game: &GameState) -> [GameObservation; 2] {
        let cheese_matrix = PyArray2::from_array(py, &self.cheese_matrix).unbind();
        let movement_matrix = self.movement_matrix.get(py, game);

        [
            Self::build_observation(
                game,
                true,
                cheese_matrix.clone_ref(py),
                movement_matrix.clone_ref(py),
            ),
            Self::build_observation(game, false, cheese_matrix, movement_matrix),
        ]
    }

    fn build_observation(
        game: &GameState,
        is_player_one: bool,
        cheese_matrix: Py<PyArray2<u8>>,
        movement_matrix: Py<PyArray3<i8>>,
    ) -> GameObservation {
        let (player_pos, player_mud, player_score) = if is_player_one {
            (
                game.player1_position(),
                game.player1.mud_timer,
                game.player1_score(),
            )
        } else {
            (
                game.player2_position(),
                game.player2.mud_timer,
                game.player2_score(),
            )
        };

        let (opponent_pos, opponent_mud, opponent_score) = if is_player_one {
            (
                game.player2_position(),
                game.player2.mud_timer,
                game.player2_score(),
            )
        } else {
            (
                game.player1_position(),
                game.player1.mud_timer,
                game.player1_score(),
            )
        };

        GameObservation {
            player_position: player_pos,
            player_mud_turns: player_mud,
            player_score,

            opponent_position: opponent_pos,
            opponent_mud_turns: opponent_mud,
            opponent_score,

            current_turn: game.turns(),
            max_turns: game.max_turns(),
            cheese_matrix,
            movement_matrix,
        }
    }

    /// Restore a single cheese position
    #[inline]
    pub fn restore_cheese(&mut self, pos: Coordinates) {
        self.cheese_matrix[[pos.x as usize, pos.y as usize]] = 1;
    }
}

/// Game observation with numpy arrays for Python
pub struct GameObservation {
    pub player_position: Coordinates,
    pub player_mud_turns: u8,
    pub player_score: f32,
    pub opponent_position: Coordinates,
    pub opponent_mud_turns: u8,
    pub opponent_score: f32,
    pub current_turn: u16,
    pub max_turns: u16,
    pub cheese_matrix: Py<PyArray2<u8>>,
    pub movement_matrix: Py<PyArray3<i8>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::builder::GameBuilder;
    use crate::game::types::MudMap;
    use std::collections::HashMap;

    #[test]
    fn test_movement_constraints() {
        let mut mud = MudMap::new();
        mud.insert(Coordinates::new(0, 0), Coordinates::new(0, 1), 2);

        let game = GameBuilder::new(3, 3)
            .with_custom_maze(HashMap::new(), mud)
            .with_corner_positions()
            .with_custom_cheese(vec![])
            .build()
            .create(None)
            .unwrap();

        let constraints = MovementConstraints::new(&game);

        // Test moves from (0,0) - bottom-left corner
        assert_eq!(
            constraints.matrix[[0, 0, Direction::Left as usize]],
            -1,
            "Left boundary should be wall"
        );
        assert_eq!(
            constraints.matrix[[0, 0, Direction::Down as usize]],
            -1,
            "Bottom boundary should be wall"
        );
        assert_eq!(
            constraints.matrix[[0, 0, Direction::Right as usize]],
            0,
            "Right move should be valid"
        );
        assert_eq!(
            constraints.matrix[[0, 0, Direction::Up as usize]],
            2,
            "Up move should have mud"
        );

        // ... rest of movement constraint tests ...
    }

    #[test]
    fn test_observation_refresh() {
        use crate::game::builder::{GameBuilder, MazeParams};

        let config = GameBuilder::new(5, 5)
            .with_random_maze(MazeParams::default())
            .with_corner_positions()
            .with_random_cheese(3, true)
            .build();
        let game = config.create(Some(42)).unwrap();
        let mut handler = ObservationHandler::new(&game);

        // Clear all cheese
        handler.cheese_matrix.fill(0);

        // Refresh should restore correct cheese positions
        handler.refresh_cheese(&game);

        for pos in game.cheese_positions() {
            assert_eq!(
                handler.cheese_matrix[[pos.x as usize, pos.y as usize]],
                1,
                "Cheese should be restored at {pos:?} after refresh"
            );
        }
    }
}
