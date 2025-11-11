//! Python bindings for the `PyRat` game engine
use crate::game::game_logic::MoveUndo;
use crate::game::observations::ObservationHandler;
use crate::{Coordinates, Direction, GameState, Wall};
use numpy::{PyArray2, PyArray3};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::Python;
use std::collections::HashMap;

use super::validation::{
    validate_cheese_positions, validate_mud, validate_optional_position, validate_wall, PyMudEntry,
    PyPosition, PyWall,
};

// Type aliases for internal Rust API (using u8)
type Position = (u8, u8);
type MudEntry = (Position, Position, u8);
type WallEntry = (Position, Position);

/// Configuration for game presets
#[derive(Clone)]
struct PresetConfig {
    width: u8,
    height: u8,
    cheese_count: u16,
    symmetric: bool,
    wall_density: f32,
    mud_density: f32,
    mud_range: u8,
    max_turns: u16,
}

impl PresetConfig {
    /// Get a preset configuration by name
    fn get_preset(name: &str) -> PyResult<Self> {
        match name {
            "tiny" => Ok(Self {
                width: 11,
                height: 9,
                cheese_count: 13,
                symmetric: true,
                wall_density: 0.7,
                mud_density: 0.1,
                mud_range: 3,
                max_turns: 150,
            }),
            "small" => Ok(Self {
                width: 15,
                height: 11,
                cheese_count: 21,
                symmetric: true,
                wall_density: 0.7,
                mud_density: 0.1,
                mud_range: 3,
                max_turns: 200,
            }),
            "default" => Ok(Self {
                width: 21,
                height: 15,
                cheese_count: 41,
                symmetric: true,
                wall_density: 0.7,
                mud_density: 0.1,
                mud_range: 3,
                max_turns: 300,
            }),
            "large" => Ok(Self {
                width: 31,
                height: 21,
                cheese_count: 85,
                symmetric: true,
                wall_density: 0.7,
                mud_density: 0.1,
                mud_range: 3,
                max_turns: 400,
            }),
            "huge" => Ok(Self {
                width: 41,
                height: 31,
                cheese_count: 165,
                symmetric: true,
                wall_density: 0.7,
                mud_density: 0.1,
                mud_range: 3,
                max_turns: 500,
            }),
            "empty" => Ok(Self {
                width: 21,
                height: 15,
                cheese_count: 41,
                symmetric: true,
                wall_density: 0.0,  // No walls
                mud_density: 0.0,   // No mud
                mud_range: 2,
                max_turns: 300,
            }),
            "asymmetric" => Ok(Self {
                width: 21,
                height: 15,
                cheese_count: 41,
                symmetric: false,  // Key difference
                wall_density: 0.7,
                mud_density: 0.1,
                mud_range: 3,
                max_turns: 300,
            }),
            _ => Err(PyValueError::new_err(format!(
                "Unknown preset '{name}'. Available presets: tiny, small, default, large, huge, empty, asymmetric"
            ))),
        }
    }
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
#[pyclass]
pub struct PyGameState {
    game: GameState,
    observation_handler: ObservationHandler,
}

#[pymethods]
impl PyGameState {
    /// Create a new game state with random generation
    #[new]
    #[pyo3(signature = (
        width=None,
        height=None,
        cheese_count=None,
        symmetric=true,
        seed=None,
        max_turns=None
    ))]
    fn new(
        width: Option<u8>,
        height: Option<u8>,
        cheese_count: Option<u16>,
        symmetric: bool,
        seed: Option<u64>,
        max_turns: Option<u16>,
    ) -> Self {
        let mut game = if symmetric {
            GameState::new_symmetric(width, height, cheese_count, seed)
        } else {
            GameState::new_asymmetric(width, height, cheese_count, seed)
        };

        // Override max_turns if provided
        if let Some(max_turns) = max_turns {
            game.max_turns = max_turns;
        }

        let observation_handler = ObservationHandler::new(&game);
        Self {
            game,
            observation_handler,
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

    // Game elements
    fn cheese_positions(&self) -> Vec<Coordinates> {
        self.game.cheese_positions()
    }

    fn mud_entries(&self) -> Vec<MudEntry> {
        self.game
            .mud_positions()
            .iter()
            .map(|((from, to), value)| ((from.x, from.y), (to.x, to.y), value))
            .collect()
    }

    /// Extract all walls from the game state
    fn wall_entries(&self) -> Vec<WallEntry> {
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
                            let wall = if (current.x, current.y) < (adjacent_pos.x, adjacent_pos.y)
                            {
                                ((current.x, current.y), (adjacent_pos.x, adjacent_pos.y))
                            } else {
                                ((adjacent_pos.x, adjacent_pos.y), (current.x, current.y))
                            };

                            // Add if not already seen
                            if seen.insert(wall) {
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
    /// Process a single game turn
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

    /// Make a move with undo capability
    fn make_move(&mut self, p1_move: u8, p2_move: u8) -> PyResult<PyMoveUndo> {
        let p1_dir = Direction::try_from(p1_move)
            .map_err(|_| PyValueError::new_err("Invalid move for player 1"))?;
        let p2_dir = Direction::try_from(p2_move)
            .map_err(|_| PyValueError::new_err("Invalid move for player 2"))?;

        let undo = self.game.make_move(p1_dir, p2_dir);
        Ok(PyMoveUndo { inner: undo })
    }

    /// Unmake a move using saved undo data
    fn unmake_move(&mut self, undo: &PyMoveUndo) {
        self.game.unmake_move(undo.inner.clone());
        // Need full refresh after unmake
        self.observation_handler.refresh_cheese(&self.game);
    }

    /// Reset the game state
    fn reset(&mut self, seed: Option<u64>) {
        self.game = GameState::new_symmetric(
            Some(self.game.width()),
            Some(self.game.height()),
            Some(self.game.total_cheese()),
            seed,
        );
        // Need full refresh after reset
        self.observation_handler.refresh_cheese(&self.game);
    }

    // String representation
    fn __repr__(&self) -> String {
        format!(
            "PyGameState({}x{}, turn={}/{}, p1_score={:.1}, p2_score={:.1})",
            self.game.width(),
            self.game.height(),
            self.game.turns(),
            self.game.max_turns(),
            self.game.player1_score(),
            self.game.player2_score()
        )
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
            cheese_matrix: obs.cheese_matrix.into_py(py),
            movement_matrix: obs.movement_matrix.into_py(py),
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
        max_turns = 300
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
    ) -> PyResult<Self> {
        // Validate and convert all inputs
        let validated_walls: Vec<Wall> = walls
            .into_iter()
            .map(|w| validate_wall(w, width, height))
            .collect::<PyResult<Vec<_>>>()?;

        let validated_mud: Vec<MudEntry> = mud
            .into_iter()
            .map(|m| validate_mud(m, width, height))
            .collect::<PyResult<Vec<_>>>()?;

        let validated_cheese = validate_cheese_positions(cheese, width, height)?;

        let validated_player1_pos =
            validate_optional_position(player1_pos, width, height, "Player 1")?;
        let validated_player2_pos =
            validate_optional_position(player2_pos, width, height, "Player 2")?;

        // Check for duplicate walls
        let mut wall_set = std::collections::HashSet::new();
        for wall in &validated_walls {
            // Wall is already normalized in validate_wall
            if !wall_set.insert((wall.pos1, wall.pos2)) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate wall between {:?} and {:?}",
                    wall.pos1, wall.pos2
                )));
            }
        }

        // Check for duplicate mud
        let mut mud_set = std::collections::HashSet::new();
        for mud_entry in &validated_mud {
            let normalized = if mud_entry.0 < mud_entry.1 {
                (mud_entry.0, mud_entry.1)
            } else {
                (mud_entry.1, mud_entry.0)
            };
            if !mud_set.insert(normalized) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate mud between {:?} and {:?}",
                    mud_entry.0, mud_entry.1
                )));
            }
        }

        // Note: Wall-mud conflicts are not checked here because mud can exist on passages
        // The maze generator ensures mud only exists on valid connections

        // Validate minimum requirements
        if validated_cheese.is_empty() {
            return Err(PyValueError::new_err("Game must have at least one cheese"));
        }

        // Now use the builder with validated data
        let builder = PyGameConfigBuilder::new(width, height);
        let mut builder_owned = builder;

        builder_owned.walls = validated_walls;
        builder_owned.mud = validated_mud;
        builder_owned.cheese = validated_cheese;
        builder_owned.player1_pos = validated_player1_pos;
        builder_owned.player2_pos = validated_player2_pos;
        builder_owned.max_turns = max_turns;

        // Build the game
        builder_owned.build()
    }

    /// Create a game from a preset configuration
    #[staticmethod]
    #[pyo3(signature = (preset="default", *, seed=None))]
    fn create_preset(preset: &str, seed: Option<u64>) -> PyResult<Self> {
        use crate::game::maze_generation::{CheeseConfig, MazeConfig};

        let config = PresetConfig::get_preset(preset)?;

        let maze_config = MazeConfig {
            width: config.width,
            height: config.height,
            target_density: config.wall_density,
            connected: true,
            symmetry: config.symmetric,
            mud_density: config.mud_density,
            mud_range: config.mud_range,
            seed,
        };

        let cheese_config = CheeseConfig {
            count: config.cheese_count,
            symmetry: config.symmetric,
        };

        let mut game =
            GameState::new_random(config.width, config.height, maze_config, cheese_config);
        game.max_turns = config.max_turns;

        let observation_handler = ObservationHandler::new(&game);
        Ok(Self {
            game,
            observation_handler,
        })
    }

    #[staticmethod]
    #[pyo3(signature = (
        width,
        height,
        walls,
        *,
        seed = None,
        max_turns = 300
    ))]
    fn create_from_maze(
        width: u8,
        height: u8,
        walls: Vec<PyWall>,
        seed: Option<u64>,
        max_turns: u16,
    ) -> PyResult<Self> {
        // Validate and convert walls
        let validated_walls: Vec<Wall> = walls
            .into_iter()
            .map(|w| validate_wall(w, width, height))
            .collect::<PyResult<Vec<_>>>()?;

        // Convert walls to adjacency list format
        let mut walls_map: HashMap<Coordinates, Vec<Coordinates>> = HashMap::new();

        // First, initialize all cells with all possible neighbors
        for x in 0..width {
            for y in 0..height {
                let coord = Coordinates { x, y };
                let mut neighbors = Vec::new();

                // Check each direction
                for &(dx, dy) in &[(0, 1), (1, 0), (0, -1), (-1, 0)] {
                    let new_x = coord.x as i8 + dx;
                    let new_y = coord.y as i8 + dy;
                    if new_x >= 0 && new_x < width as i8 && new_y >= 0 && new_y < height as i8 {
                        let neighbor = Coordinates {
                            x: new_x as u8,
                            y: new_y as u8,
                        };
                        neighbors.push(neighbor);
                    }
                }
                walls_map.insert(coord, neighbors);
            }
        }

        // Then remove connections based on walls
        for wall in validated_walls {
            let from_coord = wall.pos1;
            let to_coord = wall.pos2;

            // Remove connections in both directions
            if let Some(neighbors) = walls_map.get_mut(&from_coord) {
                neighbors.retain(|&c| c != to_coord);
            }
            if let Some(neighbors) = walls_map.get_mut(&to_coord) {
                neighbors.retain(|&c| c != from_coord);
            }
        }

        // Generate random cheese positions
        let cheese_count = ((width as u16 * height as u16) * 13 / 100).max(1); // ~13% density
        let rng_seed = seed.unwrap_or_else(|| {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        use crate::game::maze_generation::{CheeseConfig, CheeseGenerator};
        let cheese_config = CheeseConfig {
            count: cheese_count,
            symmetry: true,
        };

        let mut cheese_gen = CheeseGenerator::new(cheese_config, width, height, Some(rng_seed));

        let p1_pos = Coordinates { x: 0, y: 0 };
        let p2_pos = Coordinates {
            x: width - 1,
            y: height - 1,
        };
        let cheese_positions = cheese_gen.generate(p1_pos, p2_pos);

        // Create game with the specified walls and generated cheese
        let game = GameState::new_with_config(
            width,
            height,
            walls_map,
            HashMap::new(), // No mud
            &cheese_positions,
            Coordinates { x: 0, y: 0 }, // Default player 1 position
            Coordinates {
                x: width - 1,
                y: height - 1,
            }, // Default player 2 position
            max_turns,
        );

        let observation_handler = ObservationHandler::new(&game);
        Ok(Self {
            game,
            observation_handler,
        })
    }

    /// Create a game from a list of validated Wall objects
    #[staticmethod]
    #[pyo3(signature = (width, height, walls, *, seed=None, max_turns=300))]
    fn create_from_walls(
        width: u8,
        height: u8,
        walls: Vec<crate::Wall>,
        seed: Option<u64>,
        max_turns: u16,
    ) -> PyResult<Self> {
        // Convert walls to adjacency list format
        let mut walls_map: HashMap<Coordinates, Vec<Coordinates>> = HashMap::new();

        // Initialize all cells with neighbors
        for x in 0..width {
            for y in 0..height {
                let coord = Coordinates { x, y };
                let mut neighbors = Vec::new();
                for &(dx, dy) in &[(0, 1), (1, 0), (0, -1), (-1, 0)] {
                    let new_x = coord.x as i8 + dx;
                    let new_y = coord.y as i8 + dy;
                    if new_x >= 0 && new_x < width as i8 && new_y >= 0 && new_y < height as i8 {
                        neighbors.push(Coordinates {
                            x: new_x as u8,
                            y: new_y as u8,
                        });
                    }
                }
                walls_map.insert(coord, neighbors);
            }
        }

        // Remove connections based on walls
        for wall in walls {
            let from_coord = wall.pos1;
            let to_coord = wall.pos2;
            if let Some(neighbors) = walls_map.get_mut(&from_coord) {
                neighbors.retain(|&c| c != to_coord);
            }
            if let Some(neighbors) = walls_map.get_mut(&to_coord) {
                neighbors.retain(|&c| c != from_coord);
            }
        }

        // Generate cheese using default density and provided seed
        let rng_seed = seed.unwrap_or_else(|| {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        use crate::game::maze_generation::{CheeseConfig, CheeseGenerator};
        let cheese_config = CheeseConfig {
            count: ((width as u16 * height as u16) * 13 / 100).max(1),
            symmetry: true,
        };
        let mut cheese_gen = CheeseGenerator::new(cheese_config, width, height, Some(rng_seed));
        let p1_pos = Coordinates { x: 0, y: 0 };
        let p2_pos = Coordinates {
            x: width - 1,
            y: height - 1,
        };
        let cheese_positions = cheese_gen.generate(p1_pos, p2_pos);

        let game = GameState::new_with_config(
            width,
            height,
            walls_map,
            HashMap::new(),
            &cheese_positions,
            p1_pos,
            p2_pos,
            max_turns,
        );

        let observation_handler = ObservationHandler::new(&game);
        Ok(Self {
            game,
            observation_handler,
        })
    }

    #[staticmethod]
    #[pyo3(signature = (
        width,
        height,
        player1_start,
        player2_start,
        *,
        preset = "default",
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
        use crate::game::maze_generation::{CheeseConfig, MazeConfig};

        // Validate positions
        let p1_pos =
            validate_optional_position(Some(player1_start), width, height, "player1_start")?
                .ok_or_else(|| PyValueError::new_err("player1_start validation failed"))?;
        let p2_pos =
            validate_optional_position(Some(player2_start), width, height, "player2_start")?
                .ok_or_else(|| PyValueError::new_err("player2_start validation failed"))?;

        // Get preset configuration
        let config = PresetConfig::get_preset(preset)?;

        // Create maze with preset configuration
        let maze_config = MazeConfig {
            width,
            height,
            target_density: config.wall_density,
            connected: true,
            symmetry: config.symmetric,
            mud_density: config.mud_density,
            mud_range: config.mud_range,
            seed,
        };

        let cheese_config = CheeseConfig {
            count: ((width as u16 * height as u16) * 13 / 100).max(1), // ~13% density
            symmetry: config.symmetric,
        };

        // Generate random maze with maze generator directly
        use crate::game::maze_generation::MazeGenerator;
        let mut maze_gen = MazeGenerator::new(maze_config);
        let (walls, mud) = maze_gen.generate();

        // Generate cheese positions
        use crate::game::maze_generation::CheeseGenerator;
        let mut cheese_gen = CheeseGenerator::new(cheese_config, width, height, seed);
        let cheese_positions = cheese_gen.generate(
            Coordinates {
                x: p1_pos.0,
                y: p1_pos.1,
            },
            Coordinates {
                x: p2_pos.0,
                y: p2_pos.1,
            },
        );

        // Create game with custom positions
        let game = GameState::new_with_config(
            width,
            height,
            walls,
            (*mud).clone(),
            &cheese_positions,
            Coordinates {
                x: p1_pos.0,
                y: p1_pos.1,
            },
            Coordinates {
                x: p2_pos.0,
                y: p2_pos.1,
            },
            config.max_turns,
        );

        let observation_handler = ObservationHandler::new(&game);
        Ok(Self {
            game,
            observation_handler,
        })
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
    fn cheese_matrix<'a>(&'a self, py: Python<'a>) -> PyResult<&'a PyArray2<u8>> {
        Ok(self.cheese_matrix.as_ref(py))
    }

    #[getter]
    fn movement_matrix<'a>(&'a self, py: Python<'a>) -> PyResult<&'a PyArray3<i8>> {
        Ok(self.movement_matrix.as_ref(py))
    }
}

#[pyclass]
pub struct PyObservationHandler {
    inner: ObservationHandler,
}

#[pymethods]
impl PyObservationHandler {
    #[new]
    fn new(game: &PyGameState) -> Self {
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

    fn refresh_cheese(&mut self, game: &PyGameState) {
        self.inner.refresh_cheese(&game.game);
    }

    fn get_observation(
        &self,
        py: Python<'_>,
        game: &PyGameState,
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
            cheese_matrix: obs.cheese_matrix.into_py(py),
            movement_matrix: obs.movement_matrix.into_py(py),
        })
    }
}

#[pyclass]
pub struct PyGameConfigBuilder {
    width: u8,
    height: u8,
    walls: Vec<crate::Wall>,
    mud: Vec<MudEntry>,
    cheese: Vec<Position>,
    player1_pos: Option<Position>,
    player2_pos: Option<Position>,
    max_turns: u16,
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
        }
    }

    /// Validates that a position is within the maze bounds
    fn validate_position(&self, pos: Position, name: &str) -> PyResult<()> {
        let (x, y) = pos;
        if x >= self.width || y >= self.height {
            return Err(PyValueError::new_err(format!(
                "{} position ({}, {}) is outside maze bounds ({}x{})",
                name, x, y, self.width, self.height
            )));
        }
        Ok(())
    }

    /// Add walls to the game
    #[pyo3(name = "with_walls")]
    fn with_walls(
        mut slf: PyRefMut<'_, Self>,
        walls: Vec<crate::Wall>,
    ) -> PyResult<PyRefMut<'_, Self>> {
        // Walls are already validated by the Wall constructor
        // Just check for duplicates and mud conflicts
        for wall in &walls {
            // Check for duplicate walls
            if slf.walls.iter().any(|existing| existing == wall) {
                return Err(PyValueError::new_err(format!("Duplicate wall: {wall:?}")));
            }

            // Check for overlap with existing mud
            let pos1 = (wall.pos1.x, wall.pos1.y);
            let pos2 = (wall.pos2.x, wall.pos2.y);
            if slf.mud.iter().any(|((mx1, my1), (mx2, my2), _)| {
                (pos1 == (*mx1, *my1) && pos2 == (*mx2, *my2))
                    || (pos1 == (*mx2, *my2) && pos2 == (*mx1, *my1))
            }) {
                return Err(PyValueError::new_err(
                    "Cannot place wall where there is already mud".to_string(),
                ));
            }
        }

        slf.walls = walls;
        Ok(slf)
    }

    /// Add mud to the game
    #[pyo3(name = "with_mud")]
    fn with_mud(mut slf: PyRefMut<'_, Self>, mud: Vec<MudEntry>) -> PyResult<PyRefMut<'_, Self>> {
        for ((x1, y1), (x2, y2), value) in &mud {
            slf.validate_position((*x1, *y1), "Mud start")?;
            slf.validate_position((*x2, *y2), "Mud end")?;

            // Validate mud value (must be >= 2)
            if *value < 2 {
                return Err(PyValueError::new_err(
                    "Mud value must be at least 2 turns (1 represents normal passage)",
                ));
            }

            // Validate mud is between adjacent cells
            if !are_adjacent((*x1, *y1), (*x2, *y2)) {
                return Err(PyValueError::new_err(format!(
                    "Mud between {:?} and {:?} must be between adjacent cells",
                    (x1, y1),
                    (x2, y2)
                )));
            }

            // Check for overlap with walls
            let mud_coord1 = Coordinates::new(*x1, *y1);
            let mud_coord2 = Coordinates::new(*x2, *y2);

            if slf.walls.iter().any(|wall| {
                (wall.pos1 == mud_coord1 && wall.pos2 == mud_coord2)
                    || (wall.pos1 == mud_coord2 && wall.pos2 == mud_coord1)
            }) {
                return Err(PyValueError::new_err(format!(
                    "Cannot place mud between {:?} and {:?} where there is already a wall",
                    (x1, y1),
                    (x2, y2)
                )));
            }

            // Check for duplicate mud
            if slf.mud.iter().any(|((mx1, my1), (mx2, my2), _)| {
                (*x1, *y1) == (*mx1, *my1) && (*x2, *y2) == (*mx2, *my2)
                    || (*x1, *y1) == (*mx2, *my2) && (*x2, *y2) == (*mx1, *my1)
            }) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate mud between {:?} and {:?}",
                    (x1, y1),
                    (x2, y2)
                )));
            }
        }
        slf.mud = mud;
        Ok(slf)
    }

    /// Add mud using Mud objects
    #[pyo3(name = "with_mud_objects")]
    fn with_mud_objects(
        mut slf: PyRefMut<'_, Self>,
        mud: Vec<crate::Mud>,
    ) -> PyResult<PyRefMut<'_, Self>> {
        // Convert to internal tuple representation, reusing validations from Wall/Mud types
        let mut converted: Vec<MudEntry> = Vec::new();
        for m in mud {
            // Check overlap with walls
            if slf.walls.iter().any(|w| {
                (w.pos1 == m.pos1 && w.pos2 == m.pos2) || (w.pos1 == m.pos2 && w.pos2 == m.pos1)
            }) {
                return Err(PyValueError::new_err(format!(
                    "Cannot place mud between {:?} and {:?} where there is already a wall",
                    (m.pos1.x, m.pos1.y),
                    (m.pos2.x, m.pos2.y)
                )));
            }
            // Check duplicates
            if converted.iter().any(|((x1, y1), (x2, y2), _)| {
                (m.pos1.x, m.pos1.y) == (*x1, *y1) && (m.pos2.x, m.pos2.y) == (*x2, *y2)
                    || (m.pos1.x, m.pos1.y) == (*x2, *y2) && (m.pos2.x, m.pos2.y) == (*x1, *y1)
            }) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate mud between {:?} and {:?}",
                    (m.pos1.x, m.pos1.y),
                    (m.pos2.x, m.pos2.y)
                )));
            }
            converted.push(((m.pos1.x, m.pos1.y), (m.pos2.x, m.pos2.y), m.value));
        }
        slf.mud = converted;
        Ok(slf)
    }

    /// Add cheese positions
    #[pyo3(name = "with_cheese")]
    fn with_cheese(
        mut slf: PyRefMut<'_, Self>,
        cheese: Vec<Position>,
    ) -> PyResult<PyRefMut<'_, Self>> {
        for pos in &cheese {
            slf.validate_position(*pos, "Cheese")?;
        }

        let mut seen = std::collections::HashSet::new();
        for pos in &cheese {
            if !seen.insert(pos) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate cheese position at ({}, {})",
                    pos.0, pos.1
                )));
            }
        }

        slf.cheese = cheese;
        Ok(slf)
    }

    /// Add cheese using Coordinates objects
    #[pyo3(name = "with_cheese_coords")]
    fn with_cheese_coords(
        mut slf: PyRefMut<'_, Self>,
        cheese: Vec<Coordinates>,
    ) -> PyResult<PyRefMut<'_, Self>> {
        let mut seen = std::collections::HashSet::new();
        for pos in &cheese {
            if pos.x >= slf.width || pos.y >= slf.height {
                return Err(PyValueError::new_err(format!(
                    "Cheese position ({}, {}) is outside maze bounds ({}x{})",
                    pos.x, pos.y, slf.width, slf.height
                )));
            }
            if !seen.insert((pos.x, pos.y)) {
                return Err(PyValueError::new_err(format!(
                    "Duplicate cheese position at ({}, {})",
                    pos.x, pos.y
                )));
            }
        }
        slf.cheese = cheese.into_iter().map(|c| (c.x, c.y)).collect();
        Ok(slf)
    }

    /// Set player 1 position
    #[pyo3(name = "with_player1_pos")]
    fn with_player1_pos(
        mut slf: PyRefMut<'_, Self>,
        pos: Position,
    ) -> PyResult<PyRefMut<'_, Self>> {
        slf.validate_position(pos, "Player 1")?;
        slf.player1_pos = Some(pos);
        Ok(slf)
    }

    /// Set player 2 position
    #[pyo3(name = "with_player2_pos")]
    fn with_player2_pos(
        mut slf: PyRefMut<'_, Self>,
        pos: Position,
    ) -> PyResult<PyRefMut<'_, Self>> {
        slf.validate_position(pos, "Player 2")?;

        slf.player2_pos = Some(pos);
        Ok(slf)
    }

    /// Set player 1 position using Coordinates
    #[pyo3(name = "with_player1_pos_coords")]
    fn with_player1_pos_coords(
        mut slf: PyRefMut<'_, Self>,
        pos: Coordinates,
    ) -> PyResult<PyRefMut<'_, Self>> {
        slf.validate_position((pos.x, pos.y), "Player 1")?;
        slf.player1_pos = Some((pos.x, pos.y));
        Ok(slf)
    }

    /// Set player 2 position using Coordinates
    #[pyo3(name = "with_player2_pos_coords")]
    fn with_player2_pos_coords(
        mut slf: PyRefMut<'_, Self>,
        pos: Coordinates,
    ) -> PyResult<PyRefMut<'_, Self>> {
        slf.validate_position((pos.x, pos.y), "Player 2")?;
        slf.player2_pos = Some((pos.x, pos.y));
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
    fn build(&self) -> PyResult<PyGameState> {
        // Final validation of the complete configuration
        if self.cheese.is_empty() {
            return Err(PyValueError::new_err("Game must have at least one cheese"));
        }

        // Convert walls to HashMap
        let mut wall_map = HashMap::new();
        for wall in &self.walls {
            let pos1 = wall.pos1;
            let pos2 = wall.pos2;
            wall_map.entry(pos1).or_insert_with(Vec::new).push(pos2);
            wall_map.entry(pos2).or_insert_with(Vec::new).push(pos1);
        }

        // Convert mud to HashMap
        let mut mud_map = HashMap::new();
        for ((x1, y1), (x2, y2), value) in &self.mud {
            let pos1 = Coordinates::new(*x1, *y1);
            let pos2 = Coordinates::new(*x2, *y2);
            mud_map.insert((pos1, pos2), *value);
            mud_map.insert((pos2, pos1), *value); // Make mud symmetric
        }

        // Convert cheese positions
        let cheese_positions: Vec<_> = self
            .cheese
            .iter()
            .map(|(x, y)| Coordinates::new(*x, *y))
            .collect();

        // Create game state
        let game = GameState::new_with_config(
            self.width,
            self.height,
            wall_map,
            mud_map,
            &cheese_positions,
            self.player1_pos
                .map(|(x, y)| Coordinates::new(x, y))
                .unwrap_or_else(|| Coordinates::new(0, 0)),
            self.player2_pos
                .map(|(x, y)| Coordinates::new(x, y))
                .unwrap_or_else(|| Coordinates::new(self.width - 1, self.height - 1)),
            self.max_turns,
        );

        let observation_handler = ObservationHandler::new(&game);

        Ok(PyGameState {
            game,
            observation_handler,
        })
    }
}

// Helper function to check if two positions are adjacent
fn are_adjacent(pos1: Position, pos2: Position) -> bool {
    let dx = pos1.0.abs_diff(pos2.0);
    let dy = pos1.1.abs_diff(pos2.1);
    (dx == 1 && dy == 0) || (dx == 0 && dy == 1)
}

/// Register types submodule
pub(crate) fn register_types(m: &PyModule) -> PyResult<()> {
    m.add_class::<Coordinates>()?;
    m.add_class::<Direction>()?;
    m.add_class::<crate::Wall>()?;
    m.add_class::<crate::Mud>()?;
    Ok(())
}

/// Register game submodule
pub(crate) fn register_game(m: &PyModule) -> PyResult<()> {
    m.add_class::<PyGameState>()?;
    m.add_class::<PyMoveUndo>()?;
    Ok(())
}

/// Register observation submodule
pub(crate) fn register_observation(m: &PyModule) -> PyResult<()> {
    m.add_class::<PyGameObservation>()?;
    m.add_class::<PyObservationHandler>()?;
    Ok(())
}

/// Register builder submodule
pub(crate) fn register_builder(m: &PyModule) -> PyResult<()> {
    m.add_class::<PyGameConfigBuilder>()?;
    Ok(())
}
