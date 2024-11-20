use crate::game::observations::{ObservationHandler, GameObservation};
use crate::{Coordinates, GameState};
use numpy::{PyArray2, PyArray3};
use pyo3::prelude::*;

#[pyclass]
pub struct PyObservationHandler {
    inner: ObservationHandler,
}

#[pymethods]
impl PyObservationHandler {
    #[new]
    fn new(game: &PyGameState) -> Self {
        Self {
            inner: ObservationHandler::new(&game.game)
        }
    }

    /// Update observation after cheese collection
    fn update_collected_cheese(&mut self, collected: Vec<(u8, u8)>) {
        let coords: Vec<Coordinates> = collected
            .into_iter()
            .map(|(x, y)| Coordinates::new(x, y))
            .collect();
        self.inner.update_collected_cheese(&coords);
    }

    /// Update observation after move undo
    fn update_restored_cheese(&mut self, restored: Vec<(u8, u8)>) {
        let coords: Vec<Coordinates> = restored
            .into_iter()
            .map(|(x, y)| Coordinates::new(x, y))
            .collect();
        // Just set these positions to have cheese
        for pos in coords {
            self.inner.restore_cheese(pos);
        }
    }

    /// Get current observation
    fn get_observation<'py>(&self, py: Python<'py>, game: &PyGameState, is_player_one: bool) -> PyGameObservation {
        PyGameObservation {
            inner: self.inner.get_observation(py, &game.game, is_player_one)
        }
    }
}

#[pyclass]
pub struct PyGameObservation {
    inner: GameObservation<'static>, // We'll handle lifetimes differently
}

#[pymethods]
impl PyGameObservation {
    #[getter]
    fn player_position(&self) -> (u8, u8) {
        self.inner.player_position
    }

    #[getter]
    fn player_mud_turns(&self) -> u8 {
        self.inner.player_mud_turns
    }

    #[getter]
    fn player_score(&self) -> f32 {
        self.inner.player_score
    }

    #[getter]
    fn opponent_position(&self) -> (u8, u8) {
        self.inner.opponent_position
    }

    #[getter]
    fn opponent_mud_turns(&self) -> u8 {
        self.inner.opponent_mud_turns
    }

    #[getter]
    fn opponent_score(&self) -> f32 {
        self.inner.opponent_score
    }

    #[getter]
    fn current_turn(&self) -> u16 {
        self.inner.current_turn
    }

    #[getter]
    fn max_turns(&self) -> u16 {
        self.inner.max_turns
    }

    #[getter]
    fn cheese_matrix(&self) -> &PyArray2<u8> {
        self.inner.cheese_matrix
    }

    #[getter]
    fn movement_matrix(&self) -> &PyArray3<i8> {
        self.inner.movement_matrix
    }
}
