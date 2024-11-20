//! Python bindings for the `PyRat` game engine
use crate::game::game_logic::MoveUndo;
use crate::game::observations::ObservationHandler;
use crate::{Direction, GameState};
use numpy::{PyArray2, PyArray3};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::Python;

type MudEntry = ((u8, u8), (u8, u8), u8);
#[pyclass]
#[derive(Clone)]
pub struct PyMoveUndo {
    inner: MoveUndo,
}

#[pymethods]
impl PyMoveUndo {
    #[getter]
    fn p1_pos(&self) -> (u8, u8) {
        (self.inner.p1_pos.x, self.inner.p1_pos.y)
    }

    #[getter]
    fn p2_pos(&self) -> (u8, u8) {
        (self.inner.p2_pos.x, self.inner.p2_pos.y)
    }

    #[getter]
    fn p1_target(&self) -> (u8, u8) {
        (self.inner.p1_target.x, self.inner.p1_target.y)
    }

    #[getter]
    fn p2_target(&self) -> (u8, u8) {
        (self.inner.p2_target.x, self.inner.p2_target.y)
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
    fn collected_cheese(&self) -> Vec<(u8, u8)> {
        self.inner
            .collected_cheese
            .iter()
            .map(|pos| (pos.x, pos.y))
            .collect()
    }

    #[getter]
    fn turn(&self) -> u16 {
        self.inner.turn
    }

    fn __repr__(&self) -> String {
        format!(
            "MoveUndo(turn={}, p1_pos={:?}, p2_pos={:?}, p1_score={:.1}, p2_score={:.1})",
            self.inner.turn,
            self.p1_pos(),
            self.p2_pos(),
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
    /// Create a new game state
    #[new]
    #[pyo3(signature = (
        width=None,
        height=None,
        cheese_count=None,
        symmetric=true,
        seed=None
    ))]
    fn new(
        width: Option<u8>,
        height: Option<u8>,
        cheese_count: Option<u16>,
        symmetric: bool,
        seed: Option<u64>,
    ) -> Self {
        let game = if symmetric {
            GameState::new_symmetric(width, height, cheese_count, seed)
        } else {
            GameState::new_asymmetric(width, height, cheese_count, seed)
        };
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
    fn player1_position(&self) -> (u8, u8) {
        let pos = self.game.player1_position();
        (pos.x, pos.y)
    }

    #[getter]
    fn player2_position(&self) -> (u8, u8) {
        let pos = self.game.player2_position();
        (pos.x, pos.y)
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
    fn cheese_positions(&self) -> Vec<(u8, u8)> {
        self.game
            .cheese_positions()
            .into_iter()
            .map(|pos| (pos.x, pos.y))
            .collect()
    }

    fn mud_entries(&self) -> Vec<MudEntry> {
        self.game
            .mud_positions()
            .iter()
            .map(|((from, to), value)| ((from.x, from.y), (to.x, to.y), value))
            .collect()
    }

    // Game actions
    /// Process a single game turn
    ///
    /// Returns (game_over: bool, collected_cheese: List[(x, y)])
    fn step(&mut self, p1_move: u8, p2_move: u8) -> PyResult<(bool, Vec<(u8, u8)>)> {
        let p1_dir = Direction::try_from(p1_move)
            .map_err(|_| PyValueError::new_err("Invalid move for player 1"))?;
        let p2_dir = Direction::try_from(p2_move)
            .map_err(|_| PyValueError::new_err("Invalid move for player 2"))?;

        let result = self.game.process_turn(p1_dir, p2_dir);

        // Update only the collected cheese positions
        self.observation_handler
            .update_collected_cheese(&result.collected_cheese);

        let collected = result
            .collected_cheese
            .into_iter()
            .map(|pos| (pos.x, pos.y))
            .collect();

        Ok((result.game_over, collected))
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
}

#[pyclass]
pub struct PyGameObservation {
    player_position: (u8, u8),
    player_mud_turns: u8,
    player_score: f32,
    opponent_position: (u8, u8),
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
    fn player_position(&self) -> (u8, u8) {
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
    fn opponent_position(&self) -> (u8, u8) {
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

/// Register the module components
pub(crate) fn register_module(m: &PyModule) -> PyResult<()> {
    m.add_class::<PyGameState>()?;
    m.add_class::<PyMoveUndo>()?;
    m.add_class::<PyGameObservation>()?;
    Ok(())
}
