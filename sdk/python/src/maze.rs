use ndarray::Array3;
use numpy::PyArray3;
use pyo3::prelude::*;
use pyrat::{Coordinates, Direction, GameState};
use pyrat_engine_interface::{self as iface, GameView};
use std::collections::HashMap;

use crate::sim::PyGameSim;

/// Wire tuple for a wall: ((x1, y1), (x2, y2)).
type WallTuple = ((u8, u8), (u8, u8));

/// Wire tuple for a mud passage: ((x1, y1), (x2, y2), cost).
type MudTuple = ((u8, u8), (u8, u8), u8);

/// Wire tuple for a path result: ((x, y), path, first_moves, cost).
type PathTuple = ((u8, u8), Vec<u8>, Vec<u8>, u32);

/// Rust-backed maze graph for the Python SDK.
///
/// Constructed once from MatchConfig wire data. Provides graph queries,
/// movement matrix construction, and pathfinding — all delegating to the
/// `pyrat-engine-interface` crate.
#[pyclass]
pub struct PyMaze {
    view: GameView,
}

#[pymethods]
impl PyMaze {
    /// Build from MatchConfig wire data.
    ///
    /// - `walls`: list of ((x1,y1), (x2,y2)) pairs
    /// - `mud`: list of ((x1,y1), (x2,y2), cost) triples
    #[new]
    fn new(width: u8, height: u8, walls: Vec<WallTuple>, mud: Vec<MudTuple>) -> PyResult<Self> {
        let wall_pairs: Vec<(Coordinates, Coordinates)> = walls
            .iter()
            .map(|&((x1, y1), (x2, y2))| (Coordinates::new(x1, y1), Coordinates::new(x2, y2)))
            .collect();

        let mud_triples: Vec<(Coordinates, Coordinates, u8)> = mud
            .iter()
            .map(|&((x1, y1), (x2, y2), v)| (Coordinates::new(x1, y1), Coordinates::new(x2, y2), v))
            .collect();

        // Dummy values for fields the maze doesn't use — we only need the
        // move_table and mud_map from the GameState inside GameView.
        let p1 = Coordinates::new(0, 0);
        let p2 = Coordinates::new(width.saturating_sub(1), height.saturating_sub(1));

        let view = GameView::from_config(
            width,
            height,
            300, // max_turns — irrelevant for maze queries
            &wall_pairs,
            &mud_triples,
            vec![], // cheese — managed in Python
            p1,
            p2,
        )
        .map_err(pyo3::exceptions::PyValueError::new_err)?;

        Ok(Self { view })
    }

    /// Reachable neighbors with edge costs: list of (x, y, weight).
    fn neighbors(&self, x: u8, y: u8) -> Vec<(u8, u8, u8)> {
        self.view
            .neighbors(Coordinates::new(x, y))
            .into_iter()
            .map(|(c, w)| (c.x, c.y, w))
            .collect()
    }

    /// Edge cost between two adjacent cells, or None if walled.
    fn edge_cost(&self, x1: u8, y1: u8, x2: u8, y2: u8) -> Option<u8> {
        self.view
            .edge_cost(Coordinates::new(x1, y1), Coordinates::new(x2, y2))
    }

    /// Whether a passage exists between two cells (no wall).
    fn has_edge(&self, x1: u8, y1: u8, x2: u8, y2: u8) -> bool {
        self.view
            .has_edge(Coordinates::new(x1, y1), Coordinates::new(x2, y2))
    }

    /// Direction ints (0-3) that don't hit a wall from (x, y).
    fn effective_moves(&self, x: u8, y: u8) -> Vec<u8> {
        self.view
            .effective_moves(Coordinates::new(x, y))
            .into_iter()
            .map(|d| d as u8)
            .collect()
    }

    /// Cost of moving in a direction: None (wall), Some(1) (free), Some(N) (mud).
    fn move_cost(&self, x: u8, y: u8, direction: u8) -> PyResult<Option<u8>> {
        let dir = Direction::try_from(direction).map_err(|_| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "invalid direction {direction}, expected 0-4"
            ))
        })?;
        Ok(self.view.move_cost(Coordinates::new(x, y), dir))
    }

    /// Build the (width, height, 4) int8 movement matrix.
    ///
    /// Values: -1 = wall, 0 = free passage, N > 0 = mud cost.
    fn build_movement_matrix<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray3<i8>> {
        let maze = self.view.maze();
        let w = maze.width() as usize;
        let h = maze.height() as usize;
        let mut mat = Array3::<i8>::from_elem((w, h, 4), -1);

        for y in 0..h {
            for x in 0..w {
                let pos = Coordinates::new(x as u8, y as u8);
                for dir in [
                    Direction::Up,
                    Direction::Right,
                    Direction::Down,
                    Direction::Left,
                ] {
                    let d = dir as usize;
                    if let Some(cost) = maze.move_cost(pos, dir) {
                        // cost 1 = free (encode 0), cost > 1 = mud
                        mat[[x, y, d]] = if cost == 1 { 0 } else { cost as i8 };
                    }
                }
            }
        }

        PyArray3::from_owned_array(py, mat)
    }

    /// Shortest path: returns ((x, y), path, first_moves, cost) or None.
    fn shortest_path(&self, start: (u8, u8), goal: (u8, u8)) -> Option<PathTuple> {
        let from = Coordinates::new(start.0, start.1);
        let to = Coordinates::new(goal.0, goal.1);
        let full = iface::shortest_path_full(from, to, &self.view.maze())?;
        let path: Vec<u8> = full.path.iter().map(|d| *d as u8).collect();
        let first_moves: Vec<u8> = full.first_moves.iter().map(|d| *d as u8).collect();
        Some(((full.target.x, full.target.y), path, first_moves, full.cost))
    }

    /// Nearest cheese: returns ((x,y), path, first_moves, cost) or None.
    ///
    /// Finds the nearest cheese by Dijkstra, then reconstructs the full path to it.
    /// When multiple cheeses tie at the minimum distance, returns the first one in
    /// the cheese list — use `nearest_cheeses` to get all tied results.
    fn nearest_cheese(&self, pos: (u8, u8), cheese: Vec<(u8, u8)>) -> Option<PathTuple> {
        self.nearest_cheeses(pos, cheese).into_iter().next()
    }

    /// All cheeses tied at the minimum distance: list of ((x,y), path, first_moves, cost).
    ///
    /// Each entry has a full direction sequence from a single-pass Dijkstra.
    /// Returns an empty list if no cheese remains.
    fn nearest_cheeses(&self, pos: (u8, u8), cheese: Vec<(u8, u8)>) -> Vec<PathTuple> {
        let from = Coordinates::new(pos.0, pos.1);
        let cheese_coords: Vec<Coordinates> = cheese
            .iter()
            .map(|&(x, y)| Coordinates::new(x, y))
            .collect();

        iface::nearest_cheeses_full(from, &cheese_coords, &self.view.maze())
            .into_iter()
            .map(|full| {
                let path: Vec<u8> = full.path.iter().map(|d| *d as u8).collect();
                let first_moves: Vec<u8> = full.first_moves.iter().map(|d| *d as u8).collect();
                ((full.target.x, full.target.y), path, first_moves, full.cost)
            })
            .collect()
    }

    /// Distances from pos to all reachable cells: dict of {(x,y): cost}.
    fn distances_from(&self, pos: (u8, u8)) -> HashMap<(u8, u8), u32> {
        let from = Coordinates::new(pos.0, pos.1);
        iface::distances_from(from, &self.view.maze())
            .into_iter()
            .map(|(c, d)| ((c.x, c.y), d))
            .collect()
    }

    /// Create a mutable game snapshot for make_move / unmake_move search.
    ///
    /// The returned `GameSim` carries the maze topology (walls, mud) from
    /// this `PyMaze` and the dynamic state (positions, scores, cheese, turn)
    /// passed in from the current `GameState`.
    #[pyo3(signature = (p1_pos, p2_pos, p1_score, p2_score, p1_mud, p2_mud, cheese, turn))]
    #[allow(clippy::too_many_arguments)]
    fn simulate(
        &self,
        p1_pos: (u8, u8),
        p2_pos: (u8, u8),
        p1_score: f32,
        p2_score: f32,
        p1_mud: u8,
        p2_mud: u8,
        cheese: Vec<(u8, u8)>,
        turn: u16,
    ) -> PyGameSim {
        PyGameSim::from_maze(
            self, p1_pos, p2_pos, p1_score, p2_score, p1_mud, p2_mud, &cheese, turn,
        )
    }
}

/// Rust-only accessors (not exposed to Python).
impl PyMaze {
    pub(crate) fn snapshot(&self) -> GameState {
        self.view.snapshot()
    }
}
