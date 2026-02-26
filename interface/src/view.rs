//! SDK-facing game view — wraps a [`GameState`] with graph and pathfinding methods.
//!
//! [`GameView`] is the primary type bots interact with. It provides read-only
//! access to game state, delegates graph queries to [`Maze`], and exposes
//! pathfinding. Use [`GameView::snapshot`] for simulation (make/unmake moves
//! on a copy without affecting the view).

use crate::maze::Maze;
use crate::pathfinding;
use pyrat::{Coordinates, Direction, GameBuilder, GameState, MudMap, PlayerState};
use std::collections::HashMap;

/// Snapshot of a player's state. Copy-cheap, no references.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerSnapshot {
    pub position: Coordinates,
    pub score: f32,
    /// Turns remaining stuck in mud. 0 means the player can move freely.
    pub mud_turns: u8,
}

impl PlayerSnapshot {
    fn from_state(ps: &PlayerState) -> Self {
        Self {
            position: ps.current_pos,
            score: ps.score,
            mud_turns: ps.mud_timer,
        }
    }

    /// Whether the player is currently stuck in mud.
    pub fn is_in_mud(&self) -> bool {
        self.mud_turns > 0
    }
}

/// SDK-facing view over a game. Owns a `GameState` and delegates
/// graph queries and pathfinding to the `Maze` and pathfinding modules.
pub struct GameView {
    game: GameState,
}

impl GameView {
    // -------------------------------------------------------------------
    // Construction
    // -------------------------------------------------------------------

    /// Wrap an existing `GameState`.
    pub fn from_game(game: GameState) -> Self {
        Self { game }
    }

    /// Build from raw wire data (what SDKs receive over MatchConfig).
    ///
    /// - `walls`: pairs of adjacent cells with walls between them.
    /// - `mud`: triples of (a, b, cost) for muddy passages.
    /// - `cheese`: initial cheese positions.
    /// - `p1_start`, `p2_start`: player starting positions.
    ///
    /// # Example
    ///
    /// ```
    /// use pyrat_engine_interface::{Coordinates, GameView};
    ///
    /// let view = GameView::from_config(
    ///     5, 5, 300,
    ///     &[],  // no walls
    ///     &[],  // no mud
    ///     vec![Coordinates::new(2, 2)],
    ///     Coordinates::new(0, 0),
    ///     Coordinates::new(4, 4),
    /// ).unwrap();
    ///
    /// assert_eq!(view.width(), 5);
    /// assert_eq!(view.total_cheese(), 1);
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn from_config(
        width: u8,
        height: u8,
        max_turns: u16,
        walls: &[(Coordinates, Coordinates)],
        mud: &[(Coordinates, Coordinates, u8)],
        cheese: Vec<Coordinates>,
        p1_start: Coordinates,
        p2_start: Coordinates,
    ) -> Result<Self, String> {
        let mut wall_map: HashMap<Coordinates, Vec<Coordinates>> = HashMap::new();
        for &(a, b) in walls {
            wall_map.entry(a).or_default().push(b);
            wall_map.entry(b).or_default().push(a);
        }

        let mut mud_map = MudMap::new();
        for &(a, b, v) in mud {
            mud_map.insert(a, b, v);
        }

        let game = GameBuilder::new(width, height)
            .with_max_turns(max_turns)
            .with_custom_maze(wall_map, mud_map)
            .with_custom_positions(p1_start, p2_start)
            .with_custom_cheese(cheese)
            .build()
            .create(None)
            .map_err(|e| e.to_string())?;

        Ok(Self { game })
    }

    // -------------------------------------------------------------------
    // Player snapshots
    // -------------------------------------------------------------------

    pub fn player1(&self) -> PlayerSnapshot {
        PlayerSnapshot::from_state(&self.game.player1)
    }

    pub fn player2(&self) -> PlayerSnapshot {
        PlayerSnapshot::from_state(&self.game.player2)
    }

    // -------------------------------------------------------------------
    // Game state accessors
    // -------------------------------------------------------------------

    pub fn cheese(&self) -> Vec<Coordinates> {
        self.game.cheese.get_all_cheese_positions()
    }

    pub fn turn(&self) -> u16 {
        self.game.turn
    }

    pub fn max_turns(&self) -> u16 {
        self.game.max_turns
    }

    pub fn remaining_turns(&self) -> u16 {
        self.game.max_turns - self.game.turn
    }

    pub fn total_cheese(&self) -> u16 {
        self.game.cheese.total_cheese()
    }

    pub fn remaining_cheese(&self) -> u16 {
        self.game.cheese.remaining_cheese()
    }

    pub fn width(&self) -> u8 {
        self.game.width
    }

    pub fn height(&self) -> u8 {
        self.game.height
    }

    // -------------------------------------------------------------------
    // Maze topology
    // -------------------------------------------------------------------

    /// Borrow the static maze topology for graph queries.
    pub fn maze(&self) -> Maze<'_> {
        Maze::new(
            &self.game.move_table,
            &self.game.mud,
            self.game.width,
            self.game.height,
        )
    }

    // -------------------------------------------------------------------
    // Graph queries (delegate to Maze)
    // -------------------------------------------------------------------

    /// See [`Maze::neighbors`].
    pub fn neighbors(&self, pos: Coordinates) -> Vec<(Coordinates, u8)> {
        self.maze().neighbors(pos)
    }

    /// See [`Maze::edge_cost`].
    pub fn edge_cost(&self, a: Coordinates, b: Coordinates) -> Option<u8> {
        self.maze().edge_cost(a, b)
    }

    /// See [`Maze::has_edge`].
    pub fn has_edge(&self, a: Coordinates, b: Coordinates) -> bool {
        self.maze().has_edge(a, b)
    }

    /// See [`Maze::valid_moves`].
    pub fn valid_moves(&self, pos: Coordinates) -> Vec<Direction> {
        self.maze().valid_moves(pos)
    }

    /// See [`Maze::move_cost`].
    pub fn move_cost(&self, pos: Coordinates, dir: Direction) -> Option<u8> {
        self.maze().move_cost(pos, dir)
    }

    // -------------------------------------------------------------------
    // Pathfinding (delegate to pathfinding module)
    // -------------------------------------------------------------------

    pub fn shortest_path(
        &self,
        from: Coordinates,
        to: Coordinates,
    ) -> Option<pathfinding::PathResult> {
        pathfinding::shortest_path(from, to, &self.maze())
    }

    /// Cheeses at minimum distance from `from`, each with first-move options.
    ///
    /// # Example
    ///
    /// ```
    /// use pyrat_engine_interface::{Coordinates, GameView};
    ///
    /// let view = GameView::from_config(
    ///     5, 5, 300, &[], &[],
    ///     vec![Coordinates::new(1, 0), Coordinates::new(4, 4)],
    ///     Coordinates::new(0, 0),
    ///     Coordinates::new(4, 4),
    /// ).unwrap();
    ///
    /// let nearest = view.nearest_cheeses(Coordinates::new(0, 0));
    /// // Greedy bot: pick first move toward nearest cheese
    /// let dir = nearest[0].first_moves[0];
    /// ```
    pub fn nearest_cheeses(&self, from: Coordinates) -> Vec<pathfinding::PathResult> {
        let cheese = self.game.cheese.get_all_cheese_positions();
        pathfinding::nearest_cheeses(from, &cheese, &self.maze())
    }

    pub fn distances_from(&self, pos: Coordinates) -> HashMap<Coordinates, u32> {
        pathfinding::distances_from(pos, &self.maze())
    }

    // -------------------------------------------------------------------
    // Engine pass-through
    // -------------------------------------------------------------------

    /// Read-only access to the underlying [`GameState`].
    pub fn game(&self) -> &GameState {
        &self.game
    }

    /// Clone the game state for simulation.
    ///
    /// The returned [`GameState`] is the bot's own copy — calling `make_move`
    /// or `unmake_move` on it won't affect this view.
    ///
    /// # Example
    ///
    /// ```
    /// use pyrat_engine_interface::{Coordinates, Direction, GameView};
    ///
    /// let view = GameView::from_config(
    ///     3, 3, 100, &[], &[],
    ///     vec![Coordinates::new(1, 0)],
    ///     Coordinates::new(0, 0),
    ///     Coordinates::new(2, 2),
    /// ).unwrap();
    ///
    /// let mut sim = view.snapshot();
    /// let undo = sim.make_move(Direction::Right, Direction::Stay);
    ///
    /// // View is unchanged
    /// assert_eq!(view.player1().position, Coordinates::new(0, 0));
    /// // Snapshot moved
    /// assert_eq!(sim.player1.current_pos, Coordinates::new(1, 0));
    ///
    /// sim.unmake_move(undo);
    /// assert_eq!(sim.player1.current_pos, Coordinates::new(0, 0));
    /// ```
    pub fn snapshot(&self) -> GameState {
        self.game.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(x: u8, y: u8) -> Coordinates {
        Coordinates::new(x, y)
    }

    fn simple_view() -> GameView {
        GameView::from_config(
            5,
            5,
            300,
            &[(c(1, 0), c(1, 1))],    // one wall
            &[(c(2, 2), c(2, 3), 3)], // one mud
            vec![c(3, 3), c(4, 4)],
            c(0, 0),
            c(4, 4),
        )
        .unwrap()
    }

    #[test]
    fn from_config_dimensions() {
        let view = simple_view();
        assert_eq!(view.width(), 5);
        assert_eq!(view.height(), 5);
        assert_eq!(view.max_turns(), 300);
        assert_eq!(view.turn(), 0);
    }

    #[test]
    fn from_config_players() {
        let view = simple_view();
        let p1 = view.player1();
        let p2 = view.player2();
        assert_eq!(p1.position, c(0, 0));
        assert_eq!(p2.position, c(4, 4));
        assert_eq!(p1.score, 0.0);
        assert_eq!(p1.mud_turns, 0);
    }

    #[test]
    fn from_config_cheese() {
        let view = simple_view();
        let cheese = view.cheese();
        assert_eq!(cheese.len(), 2);
        assert!(cheese.contains(&c(3, 3)));
        assert!(cheese.contains(&c(4, 4)));
    }

    #[test]
    fn from_config_wall() {
        let view = simple_view();
        assert!(!view.has_edge(c(1, 0), c(1, 1)));
        assert!(view.has_edge(c(0, 0), c(1, 0)));
    }

    #[test]
    fn from_config_mud() {
        let view = simple_view();
        assert_eq!(view.edge_cost(c(2, 2), c(2, 3)), Some(3));
        assert_eq!(view.edge_cost(c(0, 0), c(1, 0)), Some(1));
    }

    #[test]
    fn delegation_matches_free_functions() {
        let view = simple_view();
        let maze = view.maze();

        let view_n = view.neighbors(c(0, 0));
        let maze_n = maze.neighbors(c(0, 0));
        assert_eq!(view_n, maze_n);

        let view_m = view.valid_moves(c(2, 2));
        let maze_m = maze.valid_moves(c(2, 2));
        assert_eq!(view_m, maze_m);

        assert_eq!(
            view.edge_cost(c(2, 2), c(2, 3)),
            maze.edge_cost(c(2, 2), c(2, 3))
        );
        assert_eq!(view.edge_cost(c(0, 0), c(1, 0)), Some(1));
    }

    #[test]
    fn from_game_roundtrip() {
        let game = GameBuilder::new(3, 3)
            .with_custom_maze(HashMap::new(), MudMap::new())
            .with_custom_positions(c(0, 0), c(2, 2))
            .with_custom_cheese(vec![c(1, 1)])
            .build()
            .create(None)
            .unwrap();

        let view = GameView::from_game(game);
        assert_eq!(view.width(), 3);
        assert_eq!(view.player1().position, c(0, 0));
        assert_eq!(view.cheese().len(), 1);
    }

    #[test]
    fn snapshot_make_move() {
        let view = simple_view();
        let p1_before = view.player1().position;

        // Snapshot, mutate the snapshot, verify view is unchanged
        let mut snap = view.snapshot();
        let undo = snap.make_move(Direction::Right, Direction::Stay);
        assert_ne!(snap.player1.current_pos, p1_before);

        // Original view is unaffected
        assert_eq!(view.player1().position, p1_before);

        // Undo works on snapshot
        snap.unmake_move(undo);
        assert_eq!(snap.player1.current_pos, p1_before);
    }

    #[test]
    fn from_config_empty_maze() {
        let result = GameView::from_config(3, 3, 100, &[], &[], vec![c(1, 1)], c(0, 0), c(2, 2));
        assert!(result.is_ok());
        let view = result.unwrap();
        assert_eq!(view.width(), 3);
        assert!(view.has_edge(c(0, 0), c(1, 0)));
    }

    #[test]
    fn pathfinding_through_view() {
        let view = simple_view();
        let result = view.shortest_path(c(0, 0), c(3, 3));
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.target, c(3, 3));
        assert!(r.cost > 0);
    }

    #[test]
    fn nearest_cheeses_through_view() {
        let view = simple_view();
        let results = view.nearest_cheeses(c(0, 0));
        assert!(!results.is_empty());
        assert_eq!(results[0].target, c(3, 3));
    }

    #[test]
    fn remaining_turns_at_start() {
        let view = simple_view();
        assert_eq!(view.remaining_turns(), 300);
    }

    #[test]
    fn total_and_remaining_cheese() {
        let view = simple_view();
        assert_eq!(view.total_cheese(), 2);
        assert_eq!(view.remaining_cheese(), 2);
    }

    #[test]
    fn player_snapshot_is_in_mud() {
        let view = simple_view();
        let p1 = view.player1();
        assert!(!p1.is_in_mud());
        assert_eq!(p1.mud_turns, 0);
    }
}
