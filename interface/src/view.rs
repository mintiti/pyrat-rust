use crate::{graph, pathfinding};
use pyrat::{Coordinates, Direction, GameBuilder, GameState, MudMap, PlayerState};
use std::collections::HashMap;

/// Snapshot of a player's state. Copy-cheap, no references.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Player {
    pub position: Coordinates,
    pub score: f32,
    pub mud_turns: u8,
}

impl Player {
    fn from_state(ps: &PlayerState) -> Self {
        Self {
            position: ps.current_pos,
            score: ps.score,
            mud_turns: ps.mud_timer,
        }
    }
}

/// SDK-facing view over a game. Owns a `GameState` and delegates
/// graph queries and pathfinding to the free functions in this crate.
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
    ) -> Self {
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
            .unwrap();

        Self { game }
    }

    // -------------------------------------------------------------------
    // Player snapshots
    // -------------------------------------------------------------------

    pub fn player1(&self) -> Player {
        Player::from_state(&self.game.player1)
    }

    pub fn player2(&self) -> Player {
        Player::from_state(&self.game.player2)
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

    pub fn width(&self) -> u8 {
        self.game.width
    }

    pub fn height(&self) -> u8 {
        self.game.height
    }

    // -------------------------------------------------------------------
    // Graph queries (delegate to graph module)
    // -------------------------------------------------------------------

    pub fn neighbors(&self, pos: Coordinates) -> Vec<(Coordinates, u8)> {
        graph::neighbors(pos, &self.game.move_table, &self.game.mud)
    }

    pub fn weight(&self, a: Coordinates, b: Coordinates) -> Option<u8> {
        graph::weight(a, b, &self.game.move_table, &self.game.mud)
    }

    pub fn has_edge(&self, a: Coordinates, b: Coordinates) -> bool {
        graph::has_edge(a, b, &self.game.move_table)
    }

    pub fn effective_moves(&self, pos: Coordinates) -> Vec<Direction> {
        graph::effective_moves(pos, &self.game.move_table)
    }

    pub fn move_cost(&self, pos: Coordinates, dir: Direction) -> Option<u8> {
        graph::move_cost(pos, dir, &self.game.move_table, &self.game.mud)
    }

    // -------------------------------------------------------------------
    // Pathfinding (delegate to pathfinding module)
    // -------------------------------------------------------------------

    pub fn shortest_paths(
        &self,
        from: Coordinates,
        to: Coordinates,
    ) -> Option<pathfinding::PathResult> {
        pathfinding::shortest_paths(
            from,
            to,
            self.game.width,
            self.game.height,
            &self.game.move_table,
            &self.game.mud,
        )
    }

    pub fn nearest_cheeses(&self, from: Coordinates) -> Vec<pathfinding::PathResult> {
        let cheese = self.game.cheese.get_all_cheese_positions();
        pathfinding::nearest_cheeses(
            from,
            &cheese,
            self.game.width,
            self.game.height,
            &self.game.move_table,
            &self.game.mud,
        )
    }

    pub fn distances_from(&self, pos: Coordinates) -> HashMap<Coordinates, u32> {
        pathfinding::distances_from(
            pos,
            self.game.width,
            self.game.height,
            &self.game.move_table,
            &self.game.mud,
        )
    }

    // -------------------------------------------------------------------
    // Engine pass-through
    // -------------------------------------------------------------------

    pub fn game(&self) -> &GameState {
        &self.game
    }

    pub fn game_mut(&mut self) -> &mut GameState {
        &mut self.game
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
        assert!(view.has_edge(c(0, 0), c(1, 0))); // no wall here
    }

    #[test]
    fn from_config_mud() {
        let view = simple_view();
        assert_eq!(view.weight(c(2, 2), c(2, 3)), Some(3));
        assert_eq!(view.weight(c(0, 0), c(1, 0)), Some(1)); // no mud
    }

    #[test]
    fn delegation_matches_free_functions() {
        let view = simple_view();
        let game = view.game();

        // neighbors should match
        let view_n = view.neighbors(c(0, 0));
        let free_n = graph::neighbors(c(0, 0), &game.move_table, &game.mud);
        assert_eq!(view_n, free_n);

        // effective_moves should match
        let view_m = view.effective_moves(c(2, 2));
        let free_m = graph::effective_moves(c(2, 2), &game.move_table);
        assert_eq!(view_m, free_m);
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
    fn game_mut_make_move() {
        let mut view = simple_view();
        let p1_before = view.player1().position;

        // Move player 1 right, player 2 stays
        let undo = view.game_mut().make_move(Direction::Right, Direction::Stay);

        // Player 1 should have moved
        assert_ne!(view.player1().position, p1_before);

        // Unmake should restore
        view.game_mut().unmake_move(undo);
        assert_eq!(view.player1().position, p1_before);
    }

    #[test]
    fn pathfinding_through_view() {
        let view = simple_view();
        // (0,0) to (3,3) on 5x5 grid with one wall and one mud
        let result = view.shortest_paths(c(0, 0), c(3, 3));
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
        // (3,3) should be closer than (4,4) from (0,0)
        assert_eq!(results[0].target, c(3, 3));
    }
}
