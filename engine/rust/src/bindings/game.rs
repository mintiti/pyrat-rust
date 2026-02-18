//! Python bindings for the `PyRat` game engine
use crate::game::game_logic::MoveUndo;
use crate::game::observations::ObservationHandler;
use crate::game::types::CoordinatesInput;
use crate::game::types::MudMap;
use crate::{Coordinates, Direction, GameState, Wall};
use numpy::{PyArray2, PyArray3};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::Python;
use std::collections::HashMap;

use super::validation::{
    validate_cheese_positions, validate_cheese_symmetric, validate_mud, validate_mud_symmetric,
    validate_optional_position, validate_players_symmetric, validate_wall,
    validate_walls_symmetric, PyMudEntry, PyPosition, PyWall,
};

// Type aliases for internal Rust API (using u8)
type Position = (u8, u8);
type MudEntry = (Position, Position, u8);

/// Input type for Mud that accepts either tuples or Mud objects
#[derive(FromPyObject)]
pub enum MudInput {
    /// Tuple form: ((x1, y1), (x2, y2), value)
    Tuple(PyMudEntry),
    /// Object form: Mud instance
    Object(crate::Mud),
}

/// Input type for Wall that accepts either tuples or Wall objects
#[derive(FromPyObject)]
pub enum WallInput {
    /// Tuple form: ((x1, y1), (x2, y2))
    Tuple(PyWall),
    /// Object form: Wall instance
    Object(crate::Wall),
}

/// Maze generation parameters for Python binding methods that need them
/// (e.g., `create_with_starts` which takes its own width/height).
#[derive(Clone)]
struct PresetMazeParams {
    symmetric: bool,
    wall_density: f32,
    mud_density: f32,
    mud_range: u8,
    max_turns: u16,
}

impl PresetMazeParams {
    fn get_preset(name: &str) -> PyResult<Self> {
        match name {
            "tiny" => Ok(Self { symmetric: true, wall_density: 0.7, mud_density: 0.1, mud_range: 3, max_turns: 150 }),
            "small" => Ok(Self { symmetric: true, wall_density: 0.7, mud_density: 0.1, mud_range: 3, max_turns: 200 }),
            "medium" => Ok(Self { symmetric: true, wall_density: 0.7, mud_density: 0.1, mud_range: 3, max_turns: 300 }),
            "large" => Ok(Self { symmetric: true, wall_density: 0.7, mud_density: 0.1, mud_range: 3, max_turns: 400 }),
            "huge" => Ok(Self { symmetric: true, wall_density: 0.7, mud_density: 0.1, mud_range: 3, max_turns: 500 }),
            "open" => Ok(Self { symmetric: true, wall_density: 0.0, mud_density: 0.0, mud_range: 2, max_turns: 300 }),
            "asymmetric" => Ok(Self { symmetric: false, wall_density: 0.7, mud_density: 0.1, mud_range: 3, max_turns: 300 }),
            _ => Err(PyValueError::new_err(format!(
                "Unknown preset '{name}'. Available presets: tiny, small, medium, large, huge, open, asymmetric"
            ))),
        }
    }
}

/// Convert a slice of `Wall` objects into a wall map (blocked-neighbor lists).
fn walls_to_wall_map(walls: &[Wall]) -> HashMap<Coordinates, Vec<Coordinates>> {
    let mut wall_map = HashMap::new();
    for wall in walls {
        wall_map
            .entry(wall.pos1)
            .or_insert_with(Vec::new)
            .push(wall.pos2);
        wall_map
            .entry(wall.pos2)
            .or_insert_with(Vec::new)
            .push(wall.pos1);
    }
    wall_map
}

#[pyclass]
#[derive(Clone)]
pub struct PyMoveUndo {
    inner: MoveUndo,
}

#[pymethods]
impl PyMoveUndo {
    #[getter]
    fn p1_pos(&self) -> Coordinates {
        self.inner.p1_pos
    }

    #[getter]
    fn p2_pos(&self) -> Coordinates {
        self.inner.p2_pos
    }

    #[getter]
    fn p1_target(&self) -> Coordinates {
        self.inner.p1_target
    }

    #[getter]
    fn p2_target(&self) -> Coordinates {
        self.inner.p2_target
    }

    #[getter]
    fn p1_mud(&self) -> u8 {
        self.inner.p1_mud
    }

    #[getter]
    fn p2_mud(&self) -> u8 {
        self.inner.p2_mud
    }

    #[getter]
    fn p1_score(&self) -> f32 {
        self.inner.p1_score
    }

    #[getter]
    fn p2_score(&self) -> f32 {
        self.inner.p2_score
    }

    #[getter]
    fn p1_misses(&self) -> u16 {
        self.inner.p1_misses
    }

    #[getter]
    fn p2_misses(&self) -> u16 {
        self.inner.p2_misses
    }

    #[getter]
    fn collected_cheese(&self) -> Vec<Coordinates> {
        self.inner.collected_cheese.clone()
    }

    #[getter]
    fn turn(&self) -> u16 {
        self.inner.turn
    }

    fn __repr__(&self) -> String {
        format!(
            "MoveUndo(turn={}, p1_pos={}, p2_pos={}, p1_score={:.1}, p2_score={:.1})",
            self.inner.turn,
            self.inner.p1_pos.__repr__(),
            self.inner.p2_pos.__repr__(),
            self.inner.p1_score,
            self.inner.p2_score
        )
    }
}

/// Python-facing PyRat game state
#[pyclass(name = "PyRat")]
pub struct PyRat {
    game: GameState,
    observation_handler: ObservationHandler,
    symmetric: bool,
}

#[pymethods]
impl PyRat {
    /// Create a new game state with random generation
    #[new]
    #[pyo3(signature = (
        width=None,
        height=None,
        cheese_count=None,
        symmetric=true,
        seed=None,
        max_turns=None,
        wall_density=None,
        mud_density=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        width: Option<u8>,
        height: Option<u8>,
        cheese_count: Option<u16>,
        symmetric: bool,
        seed: Option<u64>,
        max_turns: Option<u16>,
        wall_density: Option<f32>,
        mud_density: Option<f32>,
    ) -> Self {
        use crate::game::builder::{GameBuilder, MazeParams};

        let w = width.unwrap_or(GameState::DEFAULT_WIDTH);
        let h = height.unwrap_or(GameState::DEFAULT_HEIGHT);
        let cheese = cheese_count.unwrap_or(GameState::DEFAULT_CHEESE_COUNT);

        let config = GameBuilder::new(w, h)
            .with_max_turns(max_turns.unwrap_or(300))
            .with_random_maze(MazeParams {
                target_density: wall_density.unwrap_or(0.7),
                symmetry: symmetric,
                mud_density: mud_density.unwrap_or(0.1),
                ..MazeParams::default()
            })
            .with_corner_positions()
            .with_random_cheese(cheese, symmetric)
            .build();

        let game = config.create(seed);
        let observation_handler = ObservationHandler::new(&game);
        Self {
            game,
            observation_handler,
            symmetric,
        }
    }

    // Board properties
    #[getter]
    fn width(&self) -> u8 {
        self.game.width()
    }

    #[getter]
    fn height(&self) -> u8 {
        self.game.height()
    }

    // Game state
    #[getter]
    fn turn(&self) -> u16 {
        self.game.turns()
    }

    #[getter]
    fn max_turns(&self) -> u16 {
        self.game.max_turns()
    }

    // Player positions
    #[getter]
    fn player1_position(&self) -> Coordinates {
        self.game.player1_position()
    }

    #[getter]
    fn player2_position(&self) -> Coordinates {
        self.game.player2_position()
    }

    // Scores
    #[getter]
    fn player1_score(&self) -> f32 {
        self.game.player1_score()
    }

    #[getter]
    fn player2_score(&self) -> f32 {
        self.game.player2_score()
    }

    // Mud status
    #[getter]
    fn player1_mud_turns(&self) -> u8 {
        self.game.player1_mud_turns()
    }

    #[getter]
    fn player2_mud_turns(&self) -> u8 {
        self.game.player2_mud_turns()
    }

    // Game elements
    fn cheese_positions(&self) -> Vec<Coordinates> {
        self.game.cheese_positions()
    }

    /// Get valid movement directions from a position
    ///
    /// Returns a list of direction values (as integers matching Direction enum)
    /// that would result in actual movement (not blocked by walls or board boundaries).
    /// Does not include STAY.
    ///
    /// Direction values: UP=0, RIGHT=1, DOWN=2, LEFT=3
    fn get_valid_moves(&self, pos: CoordinatesInput) -> PyResult<Vec<u8>> {
        let coords: Coordinates = PyResult::<Coordinates>::from(pos)?;

        // Bounds check
        if coords.x >= self.game.width() || coords.y >= self.game.height() {
            return Err(PyValueError::new_err(format!(
                "Position ({}, {}) is outside board bounds ({}x{})",
                coords.x,
                coords.y,
                self.game.width(),
                self.game.height()
            )));
        }

        let mask = self.game.move_table.get_valid_moves(coords);
        let mut valid = Vec::with_capacity(4);

        // Bitmask: bit 0 = UP(0), bit 1 = RIGHT(1), bit 2 = DOWN(2), bit 3 = LEFT(3)
        if mask & 1 != 0 {
            valid.push(0);
        } // UP
        if mask & 2 != 0 {
            valid.push(1);
        } // RIGHT
        if mask & 4 != 0 {
            valid.push(2);
        } // DOWN
        if mask & 8 != 0 {
            valid.push(3);
        } // LEFT

        Ok(valid)
    }

    /// Get effective actions for a position (ignores mud state).
    ///
    /// Returns a list of 5 integers where result[action] = effective_action.
    /// Blocked actions (walls, boundaries) map to STAY (4).
    /// Valid actions map to themselves.
    ///
    /// Direction values: UP=0, RIGHT=1, DOWN=2, LEFT=3, STAY=4
    ///
    /// Example: at corner (0,0) with no walls
    ///   [0, 1, 4, 4, 4]  # UP=valid, RIGHT=valid, DOWN→STAY, LEFT→STAY, STAY→STAY
    fn effective_actions(&self, pos: CoordinatesInput) -> PyResult<[u8; 5]> {
        let coords: Coordinates = PyResult::<Coordinates>::from(pos)?;

        // Bounds check
        if coords.x >= self.game.width() || coords.y >= self.game.height() {
            return Err(PyValueError::new_err(format!(
                "Position ({}, {}) is outside board bounds ({}x{})",
                coords.x,
                coords.y,
                self.game.width(),
                self.game.height()
            )));
        }

        Ok(self.game.effective_actions_at(coords))
    }

    /// Get effective actions for player 1, accounting for mud state.
    ///
    /// If player 1 is in mud, all actions map to STAY [4, 4, 4, 4, 4].
    /// Otherwise, returns effective actions based on player 1's position.
    fn effective_actions_p1(&self) -> [u8; 5] {
        self.game.effective_actions_p1()
    }

    /// Get effective actions for player 2, accounting for mud state.
    ///
    /// If player 2 is in mud, all actions map to STAY [4, 4, 4, 4, 4].
    /// Otherwise, returns effective actions based on player 2's position.
    fn effective_actions_p2(&self) -> [u8; 5] {
        self.game.effective_actions_p2()
    }

    fn mud_entries(&self) -> Vec<crate::Mud> {
        self.game
            .mud_positions()
            .iter()
            .map(|((from, to), value)| {
                // Normalize order (smaller position first)
                let (p1, p2) = if from < to { (from, to) } else { (to, from) };
                crate::Mud {
                    pos1: p1,
                    pos2: p2,
                    value,
                }
            })
            .collect()
    }

    /// Extract all walls from the game state
    fn wall_entries(&self) -> Vec<crate::Wall> {
        let mut walls = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // For each position, check all adjacent cells to find walls
        for y in 0..self.game.height() {
            for x in 0..self.game.width() {
                let current = Coordinates::new(x, y);

                // Check all four adjacent cells
                let adjacent = [
                    (x.saturating_sub(1), y, x > 0, Direction::Left), // Left
                    (
                        x.saturating_add(1),
                        y,
                        x + 1 < self.game.width(),
                        Direction::Right,
                    ), // Right
                    (x, y.saturating_sub(1), y > 0, Direction::Down), // Down
                    (
                        x,
                        y.saturating_add(1),
                        y + 1 < self.game.height(),
                        Direction::Up,
                    ), // Up
                ];

                for (adj_x, adj_y, in_bounds, direction) in adjacent {
                    if in_bounds {
                        let adjacent_pos = Coordinates::new(adj_x, adj_y);

                        // Check if we can move to this adjacent cell
                        if !self.game.move_table.is_move_valid(current, direction) {
                            // Normalize wall representation (smaller position first)
                            let (p1, p2) = if current < adjacent_pos {
                                (current, adjacent_pos)
                            } else {
                                (adjacent_pos, current)
                            };

                            let wall = crate::Wall { pos1: p1, pos2: p2 };

                            // Add if not already seen
                            if seen.insert((p1, p2)) {
                                walls.push(wall);
                            }
                        }
                    }
                }
            }
        }

        walls
    }

    // Game actions
    /// Process a single game turn.
    ///
    /// Use this for straightforward game execution (playing games, collecting
    /// data, running simulations). For game tree search where you need to
    /// backtrack, use `make_move()` / `unmake_move()` instead.
    ///
    /// Returns (game_over: bool, collected_cheese: List[Coordinates])
    fn step(&mut self, p1_move: u8, p2_move: u8) -> PyResult<(bool, Vec<Coordinates>)> {
        let p1_dir = Direction::try_from(p1_move)
            .map_err(|_| PyValueError::new_err("Invalid move for player 1"))?;
        let p2_dir = Direction::try_from(p2_move)
            .map_err(|_| PyValueError::new_err("Invalid move for player 2"))?;

        let result = self.game.process_turn(p1_dir, p2_dir);

        // Update only the collected cheese positions
        self.observation_handler
            .update_collected_cheese(&result.collected_cheese);

        Ok((result.game_over, result.collected_cheese))
    }

    /// Execute a move and return undo information for backtracking.
    ///
    /// Use this (with `unmake_move()`) for game tree search algorithms
    /// like MCTS or minimax. Undo objects must be applied in LIFO order —
    /// always undo the most recent `make_move()` first.
    fn make_move(&mut self, p1_move: u8, p2_move: u8) -> PyResult<PyMoveUndo> {
        let p1_dir = Direction::try_from(p1_move)
            .map_err(|_| PyValueError::new_err("Invalid move for player 1"))?;
        let p2_dir = Direction::try_from(p2_move)
            .map_err(|_| PyValueError::new_err("Invalid move for player 2"))?;

        let undo = self.game.make_move(p1_dir, p2_dir);
        Ok(PyMoveUndo { inner: undo })
    }

    /// Revert a move using saved undo information.
    ///
    /// Restores all game state to what it was before the corresponding
    /// `make_move()` call. Undo objects must be applied in LIFO order.
    fn unmake_move(&mut self, undo: &PyMoveUndo) {
        self.game.unmake_move(undo.inner.clone());
        // Need full refresh after unmake
        self.observation_handler.refresh_cheese(&self.game);
    }

    /// Reset the game state
    #[pyo3(signature = (seed=None))]
    fn reset(&mut self, seed: Option<u64>) {
        use crate::game::builder::{GameBuilder, MazeParams};

        let config = GameBuilder::new(self.game.width(), self.game.height())
            .with_random_maze(MazeParams {
                symmetry: self.symmetric,
                ..MazeParams::default()
            })
            .with_corner_positions()
            .with_random_cheese(self.game.total_cheese(), self.symmetric)
            .build();

        self.game = config.create(seed);
        // Need full refresh after reset
        self.observation_handler.refresh_cheese(&self.game);
    }

    // String representation
    fn __repr__(&self) -> String {
        format!(
            "PyRat({}x{}, turn={}/{}, p1_score={:.1}, p2_score={:.1})",
            self.game.width(),
            self.game.height(),
            self.game.turns(),
            self.game.max_turns(),
            self.game.player1_score(),
            self.game.player2_score()
        )
    }

    // Copy protocol - enables copy.copy() and copy.deepcopy()
    fn __copy__(&self) -> Self {
        Self {
            game: self.game.clone(),
            observation_handler: self.observation_handler.clone(),
            symmetric: self.symmetric,
        }
    }

    fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.__copy__()
    }

    /// Get current observation
    pub fn get_observation(
        &self,
        py: Python<'_>,
        is_player_one: bool,
    ) -> PyResult<PyGameObservation> {
        let obs = self
            .observation_handler
            .get_observation(py, &self.game, is_player_one);

        Ok(PyGameObservation {
            player_position: obs.player_position,
            player_mud_turns: obs.player_mud_turns,
            player_score: obs.player_score,
            opponent_position: obs.opponent_position,
            opponent_mud_turns: obs.opponent_mud_turns,
            opponent_score: obs.opponent_score,
            current_turn: obs.current_turn,
            max_turns: obs.max_turns,
            cheese_matrix: obs.cheese_matrix.unbind(),
            movement_matrix: obs.movement_matrix.unbind(),
        })
    }

    /// Create a game with a fully specified maze configuration
    #[staticmethod]
    #[pyo3(signature = (
        width,
        height,
        walls = vec![],
        mud = vec![],
        cheese = vec![],
        player1_pos = None,
        player2_pos = None,
        max_turns = 300,
        symmetric = true
    ))]
    #[allow(clippy::too_many_arguments)]
    fn create_custom(
        width: u8,
        height: u8,
        walls: Vec<PyWall>,
        mud: Vec<PyMudEntry>,
        cheese: Vec<PyPosition>,
        player1_pos: Option<PyPosition>,
        player2_pos: Option<PyPosition>,
        max_turns: u16,
        symmetric: bool,
    ) -> PyResult<Self> {
        // Validate and convert all inputs
        let validated_walls: Vec<Wall> = walls
            .into_iter()
            .map(|w| validate_wall(w, width, height))
            .collect::<PyResult<Vec<_>>>()?;

        let validated_mud_tuples: Vec<MudEntry> = mud
            .into_iter()
            .map(|m| validate_mud(m, width, height))
            .collect::<PyResult<Vec<_>>>()?;

        let validated_cheese_tuples = validate_cheese_positions(cheese, width, height)?;

        let validated_player1_pos =
            validate_optional_position(player1_pos, width, height, "Player 1")?;
        let validated_player2_pos =
            validate_optional_position(player2_pos, width, height, "Player 2")?;

        // Check for duplicate walls
        let mut wall_set = std::collections::HashSet::new();
        for wall in &validated_walls {
            if !wall_set.insert((wall.pos1, wall.pos2)) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate wall between {:?} and {:?}",
                    wall.pos1, wall.pos2
                )));
            }
        }

        // Convert mud tuples to Mud objects and check for duplicates
        let mut mud_set = std::collections::HashSet::new();
        let validated_mud: Vec<crate::Mud> = validated_mud_tuples
            .into_iter()
            .map(|((x1, y1), (x2, y2), value)| {
                let (p1, p2) = if (x1, y1) < (x2, y2) {
                    (Coordinates::new(x1, y1), Coordinates::new(x2, y2))
                } else {
                    (Coordinates::new(x2, y2), Coordinates::new(x1, y1))
                };
                if !mud_set.insert((p1, p2)) {
                    return Err(PyValueError::new_err(format!(
                        "Duplicate mud between ({}, {}) and ({}, {})",
                        p1.x, p1.y, p2.x, p2.y
                    )));
                }
                Ok(crate::Mud {
                    pos1: p1,
                    pos2: p2,
                    value,
                })
            })
            .collect::<PyResult<Vec<_>>>()?;

        // Convert cheese tuples to Coordinates
        let validated_cheese: Vec<Coordinates> = validated_cheese_tuples
            .into_iter()
            .map(|(x, y)| Coordinates::new(x, y))
            .collect();

        // Validate minimum requirements
        if validated_cheese.is_empty() {
            return Err(PyValueError::new_err("Game must have at least one cheese"));
        }

        // Determine player positions for symmetry validation
        let p1_pos = validated_player1_pos
            .map(|(x, y)| Coordinates::new(x, y))
            .unwrap_or_else(|| Coordinates::new(0, 0));
        let p2_pos = validated_player2_pos
            .map(|(x, y)| Coordinates::new(x, y))
            .unwrap_or_else(|| Coordinates::new(width - 1, height - 1));

        // Validate symmetry if required
        if symmetric {
            validate_walls_symmetric(&validated_walls, width, height)
                .map_err(PyValueError::new_err)?;
            validate_mud_symmetric(&validated_mud, width, height).map_err(PyValueError::new_err)?;
            validate_cheese_symmetric(&validated_cheese, width, height)
                .map_err(PyValueError::new_err)?;
            validate_players_symmetric(p1_pos, p2_pos, width, height)
                .map_err(PyValueError::new_err)?;
        }

        // Now use the builder with validated data
        let mut builder = PyGameConfigBuilder::new(width, height);
        builder.walls = validated_walls;
        builder.mud = validated_mud;
        builder.cheese = validated_cheese;
        builder.player1_pos = validated_player1_pos.map(|(x, y)| Coordinates::new(x, y));
        builder.player2_pos = validated_player2_pos.map(|(x, y)| Coordinates::new(x, y));
        builder.max_turns = max_turns;
        builder.symmetric = symmetric;

        // Build the game
        builder.build()
    }

    /// Create a game from a preset configuration
    #[staticmethod]
    #[pyo3(signature = (preset="medium", *, seed=None))]
    fn create_preset(preset: &str, seed: Option<u64>) -> PyResult<Self> {
        use crate::game::builder::GameConfig;

        let game_config = GameConfig::preset(preset).map_err(PyValueError::new_err)?;
        let symmetric = matches!(&game_config.maze,
            crate::game::builder::MazeStrategy::Random(p) if p.symmetry);

        let game = game_config.create(seed);
        let observation_handler = ObservationHandler::new(&game);
        Ok(Self {
            game,
            observation_handler,
            symmetric,
        })
    }

    #[staticmethod]
    #[pyo3(signature = (
        width,
        height,
        walls,
        *,
        seed = None,
        max_turns = 300,
        symmetric = true
    ))]
    fn create_from_maze(
        width: u8,
        height: u8,
        walls: Vec<PyWall>,
        seed: Option<u64>,
        max_turns: u16,
        symmetric: bool,
    ) -> PyResult<Self> {
        use crate::game::builder::GameBuilder;

        let validated_walls: Vec<Wall> = walls
            .into_iter()
            .map(|w| validate_wall(w, width, height))
            .collect::<PyResult<Vec<_>>>()?;

        if symmetric {
            validate_walls_symmetric(&validated_walls, width, height)
                .map_err(PyValueError::new_err)?;
        }

        let wall_map = walls_to_wall_map(&validated_walls);
        let cheese_count = ((width as u16 * height as u16) * 13 / 100).max(1);

        let config = GameBuilder::new(width, height)
            .with_max_turns(max_turns)
            .with_custom_maze(wall_map, MudMap::new())
            .with_corner_positions()
            .with_random_cheese(cheese_count, symmetric)
            .build();

        let game = config.create(seed);
        let observation_handler = ObservationHandler::new(&game);
        Ok(Self {
            game,
            observation_handler,
            symmetric,
        })
    }

    #[staticmethod]
    #[pyo3(signature = (
        width,
        height,
        player1_start,
        player2_start,
        *,
        preset = "medium",
        seed = None
    ))]
    fn create_with_starts(
        width: u8,
        height: u8,
        player1_start: PyPosition,
        player2_start: PyPosition,
        preset: &str,
        seed: Option<u64>,
    ) -> PyResult<Self> {
        use crate::game::builder::{GameBuilder, MazeParams};

        let p1_pos =
            validate_optional_position(Some(player1_start), width, height, "player1_start")?
                .ok_or_else(|| PyValueError::new_err("player1_start validation failed"))?;
        let p2_pos =
            validate_optional_position(Some(player2_start), width, height, "player2_start")?
                .ok_or_else(|| PyValueError::new_err("player2_start validation failed"))?;

        let params = PresetMazeParams::get_preset(preset)?;
        let cheese_count = ((width as u16 * height as u16) * 13 / 100).max(1);

        let config = GameBuilder::new(width, height)
            .with_max_turns(params.max_turns)
            .with_random_maze(MazeParams {
                target_density: params.wall_density,
                connected: true,
                symmetry: params.symmetric,
                mud_density: params.mud_density,
                mud_range: params.mud_range,
            })
            .with_custom_positions(
                Coordinates::new(p1_pos.0, p1_pos.1),
                Coordinates::new(p2_pos.0, p2_pos.1),
            )
            .with_random_cheese(cheese_count, params.symmetric)
            .build();

        let game = config.create(seed);
        let observation_handler = ObservationHandler::new(&game);
        Ok(Self {
            game,
            observation_handler,
            symmetric: params.symmetric,
        })
    }
}

/// Rust-only accessors for cross-crate use (not exposed to Python)
impl PyRat {
    /// Borrow the inner `GameState`.
    pub fn game_state(&self) -> &GameState {
        &self.game
    }

    /// Wrap an existing `GameState` into a `PyRat`.
    ///
    /// `symmetric` controls how `reset()` regenerates the maze.
    pub fn from_game_state(game: GameState, symmetric: bool) -> Self {
        let observation_handler = ObservationHandler::new(&game);
        Self {
            game,
            observation_handler,
            symmetric,
        }
    }
}

#[pyclass]
pub struct PyGameObservation {
    player_position: Coordinates,
    player_mud_turns: u8,
    player_score: f32,
    opponent_position: Coordinates,
    opponent_mud_turns: u8,
    opponent_score: f32,
    current_turn: u16,
    max_turns: u16,
    cheese_matrix: Py<PyArray2<u8>>,
    movement_matrix: Py<PyArray3<i8>>,
}

#[pymethods]
impl PyGameObservation {
    #[getter]
    fn player_position(&self) -> Coordinates {
        self.player_position
    }

    #[getter]
    fn player_mud_turns(&self) -> u8 {
        self.player_mud_turns
    }

    #[getter]
    fn player_score(&self) -> f32 {
        self.player_score
    }

    #[getter]
    fn opponent_position(&self) -> Coordinates {
        self.opponent_position
    }

    #[getter]
    fn opponent_mud_turns(&self) -> u8 {
        self.opponent_mud_turns
    }

    #[getter]
    fn opponent_score(&self) -> f32 {
        self.opponent_score
    }

    #[getter]
    fn current_turn(&self) -> u16 {
        self.current_turn
    }

    #[getter]
    fn max_turns(&self) -> u16 {
        self.max_turns
    }

    #[getter]
    fn cheese_matrix<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<u8>>> {
        Ok(self.cheese_matrix.bind(py).clone())
    }

    #[getter]
    fn movement_matrix<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray3<i8>>> {
        Ok(self.movement_matrix.bind(py).clone())
    }
}

#[pyclass]
pub struct PyObservationHandler {
    inner: ObservationHandler,
}

#[pymethods]
impl PyObservationHandler {
    #[new]
    fn new(game: &PyRat) -> Self {
        Self {
            inner: ObservationHandler::new(&game.game),
        }
    }

    fn update_collected_cheese(&mut self, collected: Vec<(u8, u8)>) {
        let coords = collected
            .into_iter()
            .map(|(x, y)| Coordinates::new(x, y))
            .collect::<Vec<_>>();
        self.inner.update_collected_cheese(&coords);
    }

    fn refresh_cheese(&mut self, game: &PyRat) {
        self.inner.refresh_cheese(&game.game);
    }

    fn get_observation(
        &self,
        py: Python<'_>,
        game: &PyRat,
        is_player_one: bool,
    ) -> PyResult<PyGameObservation> {
        let obs = self.inner.get_observation(py, &game.game, is_player_one);
        Ok(PyGameObservation {
            player_position: obs.player_position,
            player_mud_turns: obs.player_mud_turns,
            player_score: obs.player_score,
            opponent_position: obs.opponent_position,
            opponent_mud_turns: obs.opponent_mud_turns,
            opponent_score: obs.opponent_score,
            current_turn: obs.current_turn,
            max_turns: obs.max_turns,
            cheese_matrix: obs.cheese_matrix.unbind(),
            movement_matrix: obs.movement_matrix.unbind(),
        })
    }
}

#[pyclass]
pub struct PyGameConfigBuilder {
    width: u8,
    height: u8,
    walls: Vec<crate::Wall>,
    mud: Vec<crate::Mud>,
    cheese: Vec<Coordinates>,
    player1_pos: Option<Coordinates>,
    player2_pos: Option<Coordinates>,
    max_turns: u16,
    symmetric: bool,
}

#[pymethods]
impl PyGameConfigBuilder {
    #[new]
    fn new(width: u8, height: u8) -> Self {
        Self {
            width,
            height,
            walls: Vec::new(),
            mud: Vec::new(),
            cheese: Vec::new(),
            player1_pos: None,
            player2_pos: None,
            max_turns: 300,
            symmetric: true,
        }
    }

    /// Validates that a Coordinates is within the maze bounds
    fn validate_coords(&self, pos: &Coordinates, name: &str) -> PyResult<()> {
        if pos.x >= self.width || pos.y >= self.height {
            return Err(PyValueError::new_err(format!(
                "{} position ({}, {}) is outside maze bounds ({}x{})",
                name, pos.x, pos.y, self.width, self.height
            )));
        }
        Ok(())
    }

    /// Add walls to the game (accepts Wall objects or tuples)
    #[pyo3(name = "with_walls")]
    fn with_walls(
        mut slf: PyRefMut<'_, Self>,
        walls: Vec<WallInput>,
    ) -> PyResult<PyRefMut<'_, Self>> {
        let mut validated_walls = Vec::new();

        for input in walls {
            let wall = match input {
                WallInput::Object(w) => w,
                WallInput::Tuple(tuple) => validate_wall(tuple, slf.width, slf.height)?,
            };

            // Check for duplicate walls
            if validated_walls
                .iter()
                .any(|existing: &crate::Wall| existing == &wall)
                || slf.walls.iter().any(|existing| existing == &wall)
            {
                return Err(PyValueError::new_err(format!("Duplicate wall: {wall:?}")));
            }

            // Check for overlap with existing mud
            if slf.mud.iter().any(|m| {
                (m.pos1 == wall.pos1 && m.pos2 == wall.pos2)
                    || (m.pos1 == wall.pos2 && m.pos2 == wall.pos1)
            }) {
                return Err(PyValueError::new_err(
                    "Cannot place wall where there is already mud".to_string(),
                ));
            }

            validated_walls.push(wall);
        }

        slf.walls = validated_walls;
        Ok(slf)
    }

    /// Add mud to the game (accepts Mud objects or tuples)
    #[pyo3(name = "with_mud")]
    fn with_mud(mut slf: PyRefMut<'_, Self>, mud: Vec<MudInput>) -> PyResult<PyRefMut<'_, Self>> {
        let mut validated_mud = Vec::new();

        for input in mud {
            let m = match input {
                MudInput::Object(m) => m,
                MudInput::Tuple(tuple) => {
                    let validated = validate_mud(tuple, slf.width, slf.height)?;
                    // Normalize order
                    let (p1, p2) = if validated.0 < validated.1 {
                        (
                            Coordinates::new(validated.0 .0, validated.0 .1),
                            Coordinates::new(validated.1 .0, validated.1 .1),
                        )
                    } else {
                        (
                            Coordinates::new(validated.1 .0, validated.1 .1),
                            Coordinates::new(validated.0 .0, validated.0 .1),
                        )
                    };
                    crate::Mud {
                        pos1: p1,
                        pos2: p2,
                        value: validated.2,
                    }
                },
            };

            // Check for overlap with walls
            if slf.walls.iter().any(|wall| {
                (wall.pos1 == m.pos1 && wall.pos2 == m.pos2)
                    || (wall.pos1 == m.pos2 && wall.pos2 == m.pos1)
            }) {
                return Err(PyValueError::new_err(format!(
                    "Cannot place mud between ({}, {}) and ({}, {}) where there is already a wall",
                    m.pos1.x, m.pos1.y, m.pos2.x, m.pos2.y
                )));
            }

            // Check for duplicate mud
            if validated_mud.iter().any(|existing: &crate::Mud| {
                (existing.pos1 == m.pos1 && existing.pos2 == m.pos2)
                    || (existing.pos1 == m.pos2 && existing.pos2 == m.pos1)
            }) || slf.mud.iter().any(|existing| {
                (existing.pos1 == m.pos1 && existing.pos2 == m.pos2)
                    || (existing.pos1 == m.pos2 && existing.pos2 == m.pos1)
            }) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate mud between ({}, {}) and ({}, {})",
                    m.pos1.x, m.pos1.y, m.pos2.x, m.pos2.y
                )));
            }

            validated_mud.push(m);
        }

        slf.mud = validated_mud;
        Ok(slf)
    }

    /// Add cheese positions (accepts Coordinates objects or tuples)
    #[pyo3(name = "with_cheese")]
    fn with_cheese(
        mut slf: PyRefMut<'_, Self>,
        cheese: Vec<CoordinatesInput>,
    ) -> PyResult<PyRefMut<'_, Self>> {
        let mut validated_cheese = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for input in cheese {
            let coord: Coordinates = PyResult::<Coordinates>::from(input)?;

            // Validate bounds
            slf.validate_coords(&coord, "Cheese")?;

            // Check for duplicates
            if !seen.insert((coord.x, coord.y)) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate cheese position at ({}, {})",
                    coord.x, coord.y
                )));
            }

            validated_cheese.push(coord);
        }

        slf.cheese = validated_cheese;
        Ok(slf)
    }

    /// Set player 1 position (accepts Coordinates or tuple)
    #[pyo3(name = "with_player1_pos")]
    fn with_player1_pos(
        mut slf: PyRefMut<'_, Self>,
        pos: CoordinatesInput,
    ) -> PyResult<PyRefMut<'_, Self>> {
        let coord: Coordinates = PyResult::<Coordinates>::from(pos)?;
        slf.validate_coords(&coord, "Player 1")?;
        slf.player1_pos = Some(coord);
        Ok(slf)
    }

    /// Set player 2 position (accepts Coordinates or tuple)
    #[pyo3(name = "with_player2_pos")]
    fn with_player2_pos(
        mut slf: PyRefMut<'_, Self>,
        pos: CoordinatesInput,
    ) -> PyResult<PyRefMut<'_, Self>> {
        let coord: Coordinates = PyResult::<Coordinates>::from(pos)?;
        slf.validate_coords(&coord, "Player 2")?;
        slf.player2_pos = Some(coord);
        Ok(slf)
    }

    /// Set maximum turns
    #[pyo3(name = "with_max_turns")]
    fn with_max_turns(mut slf: PyRefMut<'_, Self>, max_turns: u16) -> PyResult<PyRefMut<'_, Self>> {
        if max_turns == 0 {
            return Err(PyValueError::new_err("max_turns must be greater than 0"));
        }
        slf.max_turns = max_turns;
        Ok(slf)
    }

    /// Build the game state
    #[pyo3(name = "build")]
    fn build(&self) -> PyResult<PyRat> {
        use crate::game::builder::GameBuilder;

        if self.cheese.is_empty() {
            return Err(PyValueError::new_err("Game must have at least one cheese"));
        }

        let wall_map = walls_to_wall_map(&self.walls);

        let mut mud_map = MudMap::new();
        for m in &self.mud {
            mud_map.insert(m.pos1, m.pos2, m.value);
        }

        let config = GameBuilder::new(self.width, self.height)
            .with_max_turns(self.max_turns)
            .with_custom_maze(wall_map, mud_map)
            .with_custom_positions(
                self.player1_pos.unwrap_or_else(|| Coordinates::new(0, 0)),
                self.player2_pos
                    .unwrap_or_else(|| Coordinates::new(self.width - 1, self.height - 1)),
            )
            .with_custom_cheese(self.cheese.clone())
            .build();

        let game = config.create(None);
        let observation_handler = ObservationHandler::new(&game);

        Ok(PyRat {
            game,
            observation_handler,
            symmetric: self.symmetric,
        })
    }
}

/// Register types submodule
pub fn register_types(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Coordinates>()?;
    // Direction is now a Python IntEnum defined in types.py, not exposed from Rust
    m.add_class::<crate::Wall>()?;
    m.add_class::<crate::Mud>()?;
    Ok(())
}

/// Register game submodule
pub fn register_game(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyRat>()?;
    m.add_class::<PyMoveUndo>()?;
    Ok(())
}

/// Register observation submodule
pub fn register_observation(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyGameObservation>()?;
    m.add_class::<PyObservationHandler>()?;
    Ok(())
}

/// Register builder submodule
pub fn register_builder(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyGameConfigBuilder>()?;
    Ok(())
}
