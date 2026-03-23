use pyo3::prelude::*;
use pyrat::{Coordinates, Direction, GameState};

use crate::maze::PyMaze;

/// Undo token returned by [`PyGameSim::make_move`].
///
/// Holds the pre-move snapshot needed by [`PyGameSim::unmake_move`] to
/// restore state. Properties mirror the engine's `PyMoveUndo`.
#[pyclass(name = "MoveUndo")]
#[derive(Clone)]
pub struct PyMoveUndo {
    pub(crate) inner: pyrat::game::game_logic::MoveUndo,
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
            .map(|c| (c.x, c.y))
            .collect()
    }

    #[getter]
    fn turn(&self) -> u16 {
        self.inner.turn
    }

    fn __repr__(&self) -> String {
        format!(
            "MoveUndo(turn={}, p1=({},{}), p2=({},{}), score={:.1}/{:.1})",
            self.inner.turn,
            self.inner.p1_pos.x,
            self.inner.p1_pos.y,
            self.inner.p2_pos.x,
            self.inner.p2_pos.y,
            self.inner.p1_score,
            self.inner.p2_score,
        )
    }
}

/// Mutable game snapshot for make_move / unmake_move tree search.
///
/// Created via [`PyMaze::to_sim`] — no Python-facing constructor.
/// Uses objective player1/player2 naming (no my/opponent mapping).
#[pyclass(name = "GameSim")]
pub struct PyGameSim {
    game: GameState,
}

impl PyGameSim {
    /// Build from a `PyMaze`'s internal `GameView`, patching dynamic fields
    /// to match the current turn state.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_maze(
        maze: &PyMaze,
        p1_pos: (u8, u8),
        p2_pos: (u8, u8),
        p1_score: f32,
        p2_score: f32,
        p1_mud: u8,
        p2_mud: u8,
        cheese: &[(u8, u8)],
        turn: u16,
    ) -> Self {
        let mut game = maze.snapshot();

        // Patch player state.
        game.player1.current_pos = Coordinates::new(p1_pos.0, p1_pos.1);
        game.player1.score = p1_score;
        game.player1.mud_timer = p1_mud;

        game.player2.current_pos = Coordinates::new(p2_pos.0, p2_pos.1);
        game.player2.score = p2_score;
        game.player2.mud_timer = p2_mud;

        // Rebuild cheese board to match the current turn's remaining cheese.
        game.cheese.clear();
        for &(x, y) in cheese {
            game.cheese.place_cheese(Coordinates::new(x, y));
        }

        game.turn = turn;

        Self { game }
    }
}

fn parse_direction(raw: u8) -> PyResult<Direction> {
    Direction::try_from(raw).map_err(|_| {
        pyo3::exceptions::PyValueError::new_err(format!(
            "invalid direction {raw}, expected 0-4 (UP, RIGHT, DOWN, LEFT, STAY)"
        ))
    })
}

#[pymethods]
impl PyGameSim {
    /// Advance one step and return an undo token.
    ///
    /// Directions are integer-coded: UP=0, RIGHT=1, DOWN=2, LEFT=3, STAY=4.
    fn make_move(&mut self, p1_dir: u8, p2_dir: u8) -> PyResult<PyMoveUndo> {
        let d1 = parse_direction(p1_dir)?;
        let d2 = parse_direction(p2_dir)?;
        let undo = self.game.make_move(d1, d2);
        Ok(PyMoveUndo { inner: undo })
    }

    /// Revert the most recent make_move. Must be called in LIFO order.
    fn unmake_move(&mut self, undo: &PyMoveUndo) {
        self.game.unmake_move(undo.inner.clone());
    }

    // -- Properties (same names as engine's PyRat) --

    #[getter]
    fn player1_position(&self) -> (u8, u8) {
        let p = self.game.player1_position();
        (p.x, p.y)
    }

    #[getter]
    fn player2_position(&self) -> (u8, u8) {
        let p = self.game.player2_position();
        (p.x, p.y)
    }

    #[getter]
    fn player1_score(&self) -> f32 {
        self.game.player1_score()
    }

    #[getter]
    fn player2_score(&self) -> f32 {
        self.game.player2_score()
    }

    #[getter]
    fn player1_mud_turns(&self) -> u8 {
        self.game.player1_mud_turns()
    }

    #[getter]
    fn player2_mud_turns(&self) -> u8 {
        self.game.player2_mud_turns()
    }

    #[getter]
    fn cheese_positions(&self) -> Vec<(u8, u8)> {
        self.game
            .cheese_positions()
            .into_iter()
            .map(|c| (c.x, c.y))
            .collect()
    }

    #[getter]
    fn turn(&self) -> u16 {
        self.game.turns()
    }

    #[getter]
    fn max_turns(&self) -> u16 {
        self.game.max_turns()
    }

    #[getter]
    fn is_game_over(&self) -> bool {
        self.game.check_game_over()
    }

    fn __copy__(&self) -> Self {
        Self {
            game: self.game.clone(),
        }
    }

    fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Self {
        self.__copy__()
    }

    fn __repr__(&self) -> String {
        let p1 = self.game.player1_position();
        let p2 = self.game.player2_position();
        format!(
            "GameSim(turn={}/{}, p1=({},{}) {:.1}pt, p2=({},{}) {:.1}pt, cheese={})",
            self.game.turns(),
            self.game.max_turns(),
            p1.x,
            p1.y,
            self.game.player1_score(),
            p2.x,
            p2.y,
            self.game.player2_score(),
            self.game.cheese_positions().len(),
        )
    }
}
