use numpy::IntoPyArray;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::game::{game_logic::GameState, types::Direction};

type StepResult = PyResult<(Py<PyDict>, Vec<f32>, bool, bool, Py<PyDict>)>;
/// Python-facing PyRat environment
#[pyclass]
#[allow(clippy::module_name_repetitions)]
pub struct PyRatEnv {
    game: GameState,
    action_space: Vec<Direction>,
}

#[pymethods]
impl PyRatEnv {
    #[new]
    #[pyo3(signature = (
        width=Some(GameState::DEFAULT_WIDTH),
        height=Some(GameState::DEFAULT_HEIGHT),
        cheese_count=Some(GameState::DEFAULT_CHEESE_COUNT),
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

        let action_space = vec![
            Direction::Up,
            Direction::Right,
            Direction::Down,
            Direction::Left,
            Direction::Stay,
        ];

        Self { game, action_space }
    }

    /// Reset the environment to initial state
    fn reset(&mut self, seed: Option<u64>) -> PyResult<Py<PyDict>> {
        // Recreate game with same parameters but new seed
        self.game = if let Some(seed) = seed {
            GameState::new_symmetric(
                Some(self.game.width()),
                Some(self.game.height()),
                Some(self.game.total_cheese()),
                Some(seed),
            )
        } else {
            GameState::new_symmetric(
                Some(self.game.width()),
                Some(self.game.height()),
                Some(self.game.total_cheese()),
                None,
            )
        };

        // Create observation dict
        Python::with_gil(|py| {
            let obs = PyDict::new(py);
            self.fill_observation_dict(py, obs)?;
            Ok(obs.into())
        })
    }

    /// Execute one step of the environment
    #[allow(clippy::needless_pass_by_value)] // Required for PyO3 compatibility
    fn step(&mut self, actions: Vec<usize>) -> StepResult {
        if actions.len() != 2 {
            return Err(PyValueError::new_err("Must provide exactly 2 actions"));
        }

        // Convert action indices to Direction
        let p1_action = self
            .action_space
            .get(actions[0])
            .ok_or_else(|| PyValueError::new_err("Invalid action for player 1"))?;
        let p2_action = self
            .action_space
            .get(actions[1])
            .ok_or_else(|| PyValueError::new_err("Invalid action for player 2"))?;

        // Process turn
        let result = self.game.process_turn(*p1_action, *p2_action);

        // Build return values
        Python::with_gil(|py| {
            let observations = PyDict::new(py);
            self.fill_observation_dict(py, observations)?;

            let rewards = vec![result.p1_score, result.p2_score];
            let terminated = result.game_over;
            let truncated = false;

            let info = PyDict::new(py);

            Ok((
                observations.into(),
                rewards,
                terminated,
                truncated,
                info.into(),
            ))
        })
    }

    /// Get the number of possible actions
    #[getter]
    fn num_actions(&self) -> usize {
        self.action_space.len()
    }

    /// Get the current state of the game as a dictionary
    fn get_state(&self) -> PyResult<Py<PyDict>> {
        Python::with_gil(|py| {
            let obs = PyDict::new(py);
            self.fill_observation_dict(py, obs)?;
            Ok(obs.into())
        })
    }
}

impl PyRatEnv {
    /// Fill the observation dictionary with the current state
    fn fill_observation_dict(&self, py: Python<'_>, obs: &PyDict) -> PyResult<()> {
        // Add game state components
        obs.set_item("width", self.game.width())?;
        obs.set_item("height", self.game.height())?;

        // Player positions
        obs.set_item(
            "player1_pos",
            (
                self.game.player1_position().x,
                self.game.player1_position().y,
            ),
        )?;
        obs.set_item(
            "player2_pos",
            (
                self.game.player2_position().x,
                self.game.player2_position().y,
            ),
        )?;

        // Cheese positions as numpy array
        let cheese_positions = self.game.cheese_positions();
        let cheese_array: Vec<i32> = cheese_positions
            .iter()
            .flat_map(|pos| vec![i32::from(pos.x), i32::from(pos.y)])
            .collect();
        obs.set_item("cheese_positions", cheese_array.into_pyarray(py))?;

        // Mud positions and values
        let mud: Vec<(i32, i32, i32, i32, i32)> = self
            .game
            .mud_positions()
            .iter()
            .map(|((from, to), value)| {
                (
                    i32::from(from.x),
                    i32::from(from.y),
                    i32::from(to.x),
                    i32::from(to.y),
                    i32::from(*value),
                )
            })
            .collect();
        obs.set_item("mud", mud)?;

        // Scores
        obs.set_item("player1_score", self.game.player1_score())?;
        obs.set_item("player2_score", self.game.player2_score())?;

        Ok(())
    }
}

/// Register the module components
pub(crate) fn register_module(m: &PyModule) -> PyResult<()> {
    m.add_class::<PyRatEnv>()?;
    Ok(())
}
