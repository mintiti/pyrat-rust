//! Python bindings for the `PyRat` game engine
use crate::game::builder::{CheeseStrategy, GameConfig, MazeParams, MazeStrategy, PlayerStrategy};
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

use super::validation::{validate_mud, validate_wall, PyMudEntry, PyWall};

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

// ---------------------------------------------------------------------------
// PyGameConfig — reusable game configuration
// ---------------------------------------------------------------------------

/// Reusable game configuration. Stamps out `PyRat` instances via `create()`.
#[pyclass(name = "GameConfig")]
#[derive(Clone)]
pub struct PyGameConfig {
    inner: GameConfig,
}

#[pymethods]
impl PyGameConfig {
    /// Look up a named preset configuration.
    #[staticmethod]
    fn preset(name: &str) -> PyResult<Self> {
        let inner = GameConfig::preset(name).map_err(PyValueError::new_err)?;
        Ok(Self { inner })
    }

    /// Standard game: classic maze, corner starts, symmetric random cheese.
    #[staticmethod]
    fn classic(width: u8, height: u8, cheese: u16) -> Self {
        Self {
            inner: GameConfig::classic(width, height, cheese),
        }
    }

    /// Stamp out a new game from this config.
    #[pyo3(signature = (seed=None))]
    fn create(&self, seed: Option<u64>) -> PyRat {
        let game = self.inner.create(seed);
        let observation_handler = ObservationHandler::new(&game);
        PyRat {
            game,
            observation_handler,
            config: self.inner.clone(),
        }
    }

    #[getter]
    fn width(&self) -> u8 {
        self.inner.width
    }

    #[getter]
    fn height(&self) -> u8 {
        self.inner.height
    }

    #[getter]
    fn max_turns(&self) -> u16 {
        self.inner.max_turns
    }

    fn __repr__(&self) -> String {
        format!(
            "GameConfig({}x{}, max_turns={})",
            self.inner.width, self.inner.height, self.inner.max_turns
        )
    }

    fn __copy__(&self) -> Self {
        self.clone()
    }

    fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.clone()
    }
}

// ---------------------------------------------------------------------------
// PyGameBuilder — runtime-checked builder mirroring Rust typestate API
// ---------------------------------------------------------------------------

/// Builder for composing game configurations.
///
/// Enforces that maze, players, and cheese strategies are all set before
/// building. Each category must be set exactly once.
#[pyclass(name = "GameBuilder")]
pub struct PyGameBuilder {
    width: u8,
    height: u8,
    max_turns: u16,
    maze: Option<MazeStrategy>,
    players: Option<PlayerStrategy>,
    cheese: Option<CheeseStrategy>,
}

#[pymethods]
impl PyGameBuilder {
    #[new]
    fn new(width: u8, height: u8) -> PyResult<Self> {
        if width < 2 {
            return Err(PyValueError::new_err(format!(
                "width must be >= 2, got {width}"
            )));
        }
        if height < 2 {
            return Err(PyValueError::new_err(format!(
                "height must be >= 2, got {height}"
            )));
        }
        Ok(Self {
            width,
            height,
            max_turns: 300,
            maze: None,
            players: None,
            cheese: None,
        })
    }

    // -- Maze strategies --

    /// Classic maze: 0.7 wall density, 0.1 mud density, connected, symmetric.
    fn with_classic_maze(mut slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf.maze = Some(MazeStrategy::Random(MazeParams::classic()));
        slf
    }

    /// Open maze: no walls, no mud.
    fn with_open_maze(mut slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf.maze = Some(MazeStrategy::Random(MazeParams::open()));
        slf
    }

    /// Random maze with custom parameters.
    #[pyo3(signature = (*, wall_density=0.7, mud_density=0.1, mud_range=3, connected=true, symmetric=true))]
    fn with_random_maze(
        mut slf: PyRefMut<'_, Self>,
        wall_density: f32,
        mud_density: f32,
        mud_range: u8,
        connected: bool,
        symmetric: bool,
    ) -> PyRefMut<'_, Self> {
        slf.maze = Some(MazeStrategy::Random(MazeParams {
            target_density: wall_density,
            connected,
            symmetry: symmetric,
            mud_density,
            mud_range,
        }));
        slf
    }

    /// Fixed maze layout from explicit walls and mud.
    #[pyo3(signature = (walls, mud=vec![]))]
    fn with_custom_maze(
        mut slf: PyRefMut<'_, Self>,
        walls: Vec<WallInput>,
        mud: Vec<MudInput>,
    ) -> PyResult<PyRefMut<'_, Self>> {
        let width = slf.width;
        let height = slf.height;

        // Validate and convert walls
        let validated_walls: Vec<Wall> = walls
            .into_iter()
            .map(|input| match input {
                WallInput::Object(w) => Ok(w),
                WallInput::Tuple(tuple) => validate_wall(tuple, width, height),
            })
            .collect::<PyResult<Vec<_>>>()?;

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

        // Validate and convert mud
        let mut mud_set = std::collections::HashSet::new();
        let mut mud_map = MudMap::new();
        for input in mud {
            let m = match input {
                MudInput::Object(m) => m,
                MudInput::Tuple(tuple) => {
                    let validated = validate_mud(tuple, width, height)?;
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
            let key = if m.pos1 < m.pos2 {
                (m.pos1, m.pos2)
            } else {
                (m.pos2, m.pos1)
            };
            if !mud_set.insert(key) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate mud between ({}, {}) and ({}, {})",
                    m.pos1.x, m.pos1.y, m.pos2.x, m.pos2.y
                )));
            }
            mud_map.insert(m.pos1, m.pos2, m.value);
        }

        let wall_map = walls_to_wall_map(&validated_walls);
        slf.maze = Some(MazeStrategy::Fixed {
            walls: wall_map,
            mud: mud_map,
        });
        Ok(slf)
    }

    // -- Player strategies --

    /// Player 1 at (0,0), player 2 at (width-1, height-1).
    fn with_corner_positions(mut slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf.players = Some(PlayerStrategy::Corners);
        slf
    }

    /// Both players placed randomly (guaranteed different cells).
    fn with_random_positions(mut slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf.players = Some(PlayerStrategy::Random);
        slf
    }

    /// Place players at explicit positions.
    fn with_custom_positions(
        mut slf: PyRefMut<'_, Self>,
        p1: CoordinatesInput,
        p2: CoordinatesInput,
    ) -> PyResult<PyRefMut<'_, Self>> {
        let p1_coord: Coordinates = PyResult::<Coordinates>::from(p1)?;
        let p2_coord: Coordinates = PyResult::<Coordinates>::from(p2)?;

        if p1_coord.x >= slf.width || p1_coord.y >= slf.height {
            return Err(PyValueError::new_err(format!(
                "Player 1 position ({}, {}) is outside board bounds ({}x{})",
                p1_coord.x, p1_coord.y, slf.width, slf.height
            )));
        }
        if p2_coord.x >= slf.width || p2_coord.y >= slf.height {
            return Err(PyValueError::new_err(format!(
                "Player 2 position ({}, {}) is outside board bounds ({}x{})",
                p2_coord.x, p2_coord.y, slf.width, slf.height
            )));
        }

        slf.players = Some(PlayerStrategy::Fixed(p1_coord, p2_coord));
        Ok(slf)
    }

    // -- Cheese strategies --

    /// Place `count` cheese randomly, optionally with 180° symmetry.
    #[pyo3(signature = (count, symmetric=true))]
    fn with_random_cheese(
        mut slf: PyRefMut<'_, Self>,
        count: u16,
        symmetric: bool,
    ) -> PyRefMut<'_, Self> {
        slf.cheese = Some(CheeseStrategy::Random { count, symmetric });
        slf
    }

    /// Place cheese at exact positions.
    fn with_custom_cheese(
        mut slf: PyRefMut<'_, Self>,
        positions: Vec<CoordinatesInput>,
    ) -> PyResult<PyRefMut<'_, Self>> {
        let mut validated = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for input in positions {
            let coord: Coordinates = PyResult::<Coordinates>::from(input)?;
            if coord.x >= slf.width || coord.y >= slf.height {
                return Err(PyValueError::new_err(format!(
                    "Cheese position ({}, {}) is outside board bounds ({}x{})",
                    coord.x, coord.y, slf.width, slf.height
                )));
            }
            if !seen.insert((coord.x, coord.y)) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate cheese position at ({}, {})",
                    coord.x, coord.y
                )));
            }
            validated.push(coord);
        }

        if validated.is_empty() {
            return Err(PyValueError::new_err("Game must have at least one cheese"));
        }

        slf.cheese = Some(CheeseStrategy::Fixed(validated));
        Ok(slf)
    }

    // -- Other --

    /// Override the default max_turns (300).
    fn with_max_turns(mut slf: PyRefMut<'_, Self>, max_turns: u16) -> PyResult<PyRefMut<'_, Self>> {
        if max_turns == 0 {
            return Err(PyValueError::new_err("max_turns must be greater than 0"));
        }
        slf.max_turns = max_turns;
        Ok(slf)
    }

    /// Consume the builder and produce a `GameConfig`.
    fn build(&self) -> PyResult<PyGameConfig> {
        let maze = self.maze.clone().ok_or_else(|| {
            PyValueError::new_err(
                "Maze strategy not set. Call with_classic_maze(), with_open_maze(), \
                 with_random_maze(), or with_custom_maze() before build().",
            )
        })?;
        let players = self.players.clone().ok_or_else(|| {
            PyValueError::new_err(
                "Player strategy not set. Call with_corner_positions(), \
                 with_random_positions(), or with_custom_positions() before build().",
            )
        })?;
        let cheese = self.cheese.clone().ok_or_else(|| {
            PyValueError::new_err(
                "Cheese strategy not set. Call with_random_cheese() or \
                 with_custom_cheese() before build().",
            )
        })?;

        Ok(PyGameConfig {
            inner: GameConfig {
                width: self.width,
                height: self.height,
                max_turns: self.max_turns,
                maze,
                players,
                cheese,
            },
        })
    }
}

// ---------------------------------------------------------------------------
// PyRat — Python-facing game state
// ---------------------------------------------------------------------------

/// Python-facing PyRat game state
#[pyclass(name = "PyRat")]
pub struct PyRat {
    game: GameState,
    observation_handler: ObservationHandler,
    config: GameConfig,
}

#[pymethods]
impl PyRat {
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

    /// Reset the game state using the stored config.
    #[pyo3(signature = (seed=None))]
    fn reset(&mut self, seed: Option<u64>) {
        self.game = self.config.create(seed);
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
            config: self.config.clone(),
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
}

/// Rust-only accessors for cross-crate use (not exposed to Python)
impl PyRat {
    /// Borrow the inner `GameState`.
    pub fn game_state(&self) -> &GameState {
        &self.game
    }

    /// Wrap an existing `GameState` into a `PyRat` with a config for `reset()`.
    pub fn from_game_state(game: GameState, config: GameConfig) -> Self {
        let observation_handler = ObservationHandler::new(&game);
        Self {
            game,
            observation_handler,
            config,
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
    m.add_class::<PyGameBuilder>()?;
    m.add_class::<PyGameConfig>()?;
    Ok(())
}
