//! Static maze topology — the graph structure that doesn't change during a game.
//!
//! [`Maze`] bundles the move table, mud map, and dimensions into a single borrow.
//! All graph queries (neighbors, edge costs, valid moves) are methods on `Maze`.

use pyrat::{Coordinates, Direction, MoveTable, MudMap};

/// Borrow-bundle for the static maze topology.
///
/// The maze structure — walls, mud, dimensions — is fixed at game start.
/// `Maze` borrows these from a [`GameState`](pyrat::GameState) and provides
/// graph query methods.
#[derive(Clone, Copy)]
pub struct Maze<'a> {
    move_table: &'a MoveTable,
    mud: &'a MudMap,
    width: u8,
    height: u8,
}

impl<'a> Maze<'a> {
    pub fn new(move_table: &'a MoveTable, mud: &'a MudMap, width: u8, height: u8) -> Self {
        Self {
            move_table,
            mud,
            width,
            height,
        }
    }

    pub fn width(&self) -> u8 {
        self.width
    }

    pub fn height(&self) -> u8 {
        self.height
    }

    pub fn size(&self) -> usize {
        self.width as usize * self.height as usize
    }

    pub fn move_table(&self) -> &MoveTable {
        self.move_table
    }

    pub fn mud(&self) -> &MudMap {
        self.mud
    }

    /// Adjacent walkable cells with edge costs.
    /// Cost is 1 for free passage, N for mud (N >= 2).
    pub fn neighbors(&self, pos: Coordinates) -> Vec<(Coordinates, u8)> {
        self.move_table
            .valid_directions(pos)
            .map(|dir| {
                let neighbor = dir.apply_to(pos);
                let w = self.mud.get(pos, neighbor).unwrap_or(1);
                (neighbor, w)
            })
            .collect()
    }

    /// Edge cost between two adjacent cells.
    /// Returns `None` if there's a wall or cells aren't adjacent.
    /// 1 = free passage, N = mud (takes N turns to traverse).
    pub fn edge_cost(&self, a: Coordinates, b: Coordinates) -> Option<u8> {
        let dir = Direction::between(a, b)?;
        if self.move_table.is_move_valid(a, dir) {
            Some(self.mud.get(a, b).unwrap_or(1))
        } else {
            None
        }
    }

    /// Is there a passage between two cells? (no wall)
    pub fn has_edge(&self, a: Coordinates, b: Coordinates) -> bool {
        Direction::between(a, b).is_some_and(|dir| self.move_table.is_move_valid(a, dir))
    }

    /// Directions from `pos` that lead to a valid cell (not into walls or boundaries).
    pub fn valid_moves(&self, pos: Coordinates) -> Vec<Direction> {
        self.move_table.valid_directions(pos).collect()
    }

    /// Cost of moving in a specific direction from `pos`.
    /// `None` if the move hits a wall or boundary. `None` for [`Direction::Stay`].
    /// 1 = free, N = mud (takes N turns to traverse).
    pub fn move_cost(&self, pos: Coordinates, dir: Direction) -> Option<u8> {
        if !self.move_table.is_move_valid(pos, dir) {
            return None;
        }
        let dest = dir.apply_to(pos);
        Some(self.mud.get(pos, dest).unwrap_or(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat::GameBuilder;
    use std::collections::HashMap;

    fn c(x: u8, y: u8) -> Coordinates {
        Coordinates::new(x, y)
    }

    /// 3x3 open grid, no walls, no mud.
    fn open_3x3() -> (MoveTable, MudMap) {
        let game = GameBuilder::new(3, 3)
            .with_custom_maze(HashMap::new(), MudMap::new())
            .with_custom_positions(c(0, 0), c(2, 2))
            .with_custom_cheese(vec![c(1, 1)])
            .build()
            .create(None)
            .unwrap();
        (game.move_table, game.mud)
    }

    /// 3x3 grid with vertical wall between x=0 and x=1 at y=1.
    fn walled_3x3() -> (MoveTable, MudMap) {
        let mut walls: HashMap<Coordinates, Vec<Coordinates>> = HashMap::new();
        walls.entry(c(0, 1)).or_default().push(c(1, 1));
        walls.entry(c(1, 1)).or_default().push(c(0, 1));

        let game = GameBuilder::new(3, 3)
            .with_custom_maze(walls, MudMap::new())
            .with_custom_positions(c(0, 0), c(2, 2))
            .with_custom_cheese(vec![c(1, 1)])
            .build()
            .create(None)
            .unwrap();
        (game.move_table, game.mud)
    }

    /// 3x3 grid with mud between (1,0) and (1,1) of weight 3.
    fn muddy_3x3() -> (MoveTable, MudMap) {
        let mut mud = MudMap::new();
        mud.insert(c(1, 0), c(1, 1), 3);

        let game = GameBuilder::new(3, 3)
            .with_custom_maze(HashMap::new(), mud)
            .with_custom_positions(c(0, 0), c(2, 2))
            .with_custom_cheese(vec![c(1, 1)])
            .build()
            .create(None)
            .unwrap();
        (game.move_table, game.mud)
    }

    // -------------------------------------------------------------------
    // neighbors
    // -------------------------------------------------------------------

    #[test]
    fn neighbors_center_open_grid() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        let mut n = maze.neighbors(c(1, 1));
        n.sort_by_key(|(c, _)| (c.x, c.y));

        assert_eq!(n.len(), 4);
        assert!(n.iter().all(|(_, w)| *w == 1));
    }

    #[test]
    fn neighbors_corner_open_grid() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        let n = maze.neighbors(c(0, 0));
        assert_eq!(n.len(), 2);
    }

    #[test]
    fn neighbors_with_wall() {
        let (mt, mud) = walled_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        let n = maze.neighbors(c(0, 1));
        assert_eq!(n.len(), 2);
        let coords: Vec<_> = n.iter().map(|(c, _)| *c).collect();
        assert!(!coords.contains(&c(1, 1)));
    }

    #[test]
    fn neighbors_with_mud() {
        let (mt, mud) = muddy_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        let n = maze.neighbors(c(1, 0));
        let muddy_neighbor = n.iter().find(|(c, _)| *c == Coordinates::new(1, 1));
        assert_eq!(muddy_neighbor, Some(&(c(1, 1), 3)));
    }

    // -------------------------------------------------------------------
    // edge_cost
    // -------------------------------------------------------------------

    #[test]
    fn edge_cost_open() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert_eq!(maze.edge_cost(c(0, 0), c(1, 0)), Some(1));
    }

    #[test]
    fn edge_cost_walled() {
        let (mt, mud) = walled_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert_eq!(maze.edge_cost(c(0, 1), c(1, 1)), None);
    }

    #[test]
    fn edge_cost_mud() {
        let (mt, mud) = muddy_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert_eq!(maze.edge_cost(c(1, 0), c(1, 1)), Some(3));
    }

    #[test]
    fn edge_cost_non_adjacent() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert_eq!(maze.edge_cost(c(0, 0), c(2, 2)), None);
    }

    // -------------------------------------------------------------------
    // has_edge
    // -------------------------------------------------------------------

    #[test]
    fn has_edge_open() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert!(maze.has_edge(c(0, 0), c(1, 0)));
        assert!(maze.has_edge(c(1, 0), c(0, 0)));
    }

    #[test]
    fn has_edge_walled() {
        let (mt, mud) = walled_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert!(!maze.has_edge(c(0, 1), c(1, 1)));
        assert!(!maze.has_edge(c(1, 1), c(0, 1)));
    }

    #[test]
    fn has_edge_non_adjacent() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert!(!maze.has_edge(c(0, 0), c(2, 2)));
    }

    // -------------------------------------------------------------------
    // valid_moves
    // -------------------------------------------------------------------

    #[test]
    fn valid_moves_center() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        let moves = maze.valid_moves(c(1, 1));
        assert_eq!(moves.len(), 4);
    }

    #[test]
    fn valid_moves_corner() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        let moves = maze.valid_moves(c(0, 0));
        assert_eq!(moves.len(), 2);
        assert!(moves.contains(&Direction::Up));
        assert!(moves.contains(&Direction::Right));
    }

    #[test]
    fn valid_moves_with_wall() {
        let (mt, mud) = walled_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        let moves = maze.valid_moves(c(0, 1));
        assert_eq!(moves.len(), 2);
        assert!(!moves.contains(&Direction::Right));
    }

    // -------------------------------------------------------------------
    // move_cost
    // -------------------------------------------------------------------

    #[test]
    fn move_cost_open() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert_eq!(maze.move_cost(c(1, 1), Direction::Up), Some(1));
    }

    #[test]
    fn move_cost_wall() {
        let (mt, mud) = walled_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert_eq!(maze.move_cost(c(0, 1), Direction::Right), None);
    }

    #[test]
    fn move_cost_boundary() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert_eq!(maze.move_cost(c(0, 0), Direction::Down), None);
    }

    #[test]
    fn move_cost_mud() {
        let (mt, mud) = muddy_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert_eq!(maze.move_cost(c(1, 0), Direction::Up), Some(3));
    }

    #[test]
    fn move_cost_stay_returns_none() {
        let (mt, mud) = open_3x3();
        let maze = Maze::new(&mt, &mud, 3, 3);
        assert_eq!(maze.move_cost(c(1, 1), Direction::Stay), None);
    }
}
