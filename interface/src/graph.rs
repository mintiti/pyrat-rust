use pyrat::{Coordinates, Direction, MoveTable, MudMap};

/// All four movement directions (excludes Stay).
const DIRECTIONS: [Direction; 4] = [
    Direction::Up,
    Direction::Right,
    Direction::Down,
    Direction::Left,
];

/// Derive the direction needed to move from `a` to `b`.
/// Returns None if the cells aren't orthogonally adjacent.
pub fn direction_between(a: Coordinates, b: Coordinates) -> Option<Direction> {
    let dx = b.x as i16 - a.x as i16;
    let dy = b.y as i16 - a.y as i16;
    match (dx, dy) {
        (0, 1) => Some(Direction::Up),
        (0, -1) => Some(Direction::Down),
        (1, 0) => Some(Direction::Right),
        (-1, 0) => Some(Direction::Left),
        _ => None,
    }
}

/// Adjacent walkable cells with edge weights.
/// Weight is 1 for free passage, N for mud (N >= 2).
pub fn neighbors(pos: Coordinates, move_table: &MoveTable, mud: &MudMap) -> Vec<(Coordinates, u8)> {
    let mask = move_table.get_valid_moves(pos);
    let mut result = Vec::with_capacity(4);

    for (bit, dir) in DIRECTIONS.iter().enumerate() {
        if mask & (1 << bit) != 0 {
            let neighbor = dir.apply_to(pos);
            let w = mud.get(pos, neighbor).unwrap_or(1);
            result.push((neighbor, w));
        }
    }

    result
}

/// Edge weight between two adjacent cells.
/// Returns None if there's a wall (or cells aren't adjacent).
/// 1 = free passage, N = mud turns.
pub fn weight(a: Coordinates, b: Coordinates, move_table: &MoveTable, mud: &MudMap) -> Option<u8> {
    let dir = direction_between(a, b)?;
    if move_table.is_move_valid(a, dir) {
        Some(mud.get(a, b).unwrap_or(1))
    } else {
        None
    }
}

/// Is there a passage between two cells? (no wall)
pub fn has_edge(a: Coordinates, b: Coordinates, move_table: &MoveTable) -> bool {
    direction_between(a, b).is_some_and(|dir| move_table.is_move_valid(a, dir))
}

/// Directions from `pos` that actually move you (not into walls/boundaries).
pub fn effective_moves(pos: Coordinates, move_table: &MoveTable) -> Vec<Direction> {
    let mask = move_table.get_valid_moves(pos);
    let mut result = Vec::with_capacity(4);

    for (bit, dir) in DIRECTIONS.iter().enumerate() {
        if mask & (1 << bit) != 0 {
            result.push(*dir);
        }
    }

    result
}

/// Cost of moving in a specific direction from `pos`.
/// None if the move hits a wall or boundary.
/// 1 = free, N = mud turns.
pub fn move_cost(
    pos: Coordinates,
    dir: Direction,
    move_table: &MoveTable,
    mud: &MudMap,
) -> Option<u8> {
    if !move_table.is_move_valid(pos, dir) {
        return None;
    }
    let dest = dir.apply_to(pos);
    Some(mud.get(pos, dest).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat::GameBuilder;
    use std::collections::HashMap;

    /// 3x3 open grid, no walls, no mud.
    fn open_3x3() -> (MoveTable, MudMap) {
        let game = GameBuilder::new(3, 3)
            .with_custom_maze(HashMap::new(), MudMap::new())
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build()
            .create(None)
            .unwrap();
        (game.move_table, game.mud)
    }

    /// 3x3 grid with vertical wall between x=0 and x=1 at y=1.
    fn walled_3x3() -> (MoveTable, MudMap) {
        let mut walls: HashMap<Coordinates, Vec<Coordinates>> = HashMap::new();
        // Wall between (0,1) and (1,1)
        walls
            .entry(Coordinates::new(0, 1))
            .or_default()
            .push(Coordinates::new(1, 1));
        walls
            .entry(Coordinates::new(1, 1))
            .or_default()
            .push(Coordinates::new(0, 1));

        let game = GameBuilder::new(3, 3)
            .with_custom_maze(walls, MudMap::new())
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build()
            .create(None)
            .unwrap();
        (game.move_table, game.mud)
    }

    /// 3x3 grid with mud between (1,0) and (1,1) of weight 3.
    fn muddy_3x3() -> (MoveTable, MudMap) {
        let mut mud = MudMap::new();
        mud.insert(Coordinates::new(1, 0), Coordinates::new(1, 1), 3);

        let game = GameBuilder::new(3, 3)
            .with_custom_maze(HashMap::new(), mud)
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build()
            .create(None)
            .unwrap();
        (game.move_table, game.mud)
    }

    #[test]
    fn direction_between_adjacent() {
        let a = Coordinates::new(1, 1);
        assert_eq!(
            direction_between(a, Coordinates::new(1, 2)),
            Some(Direction::Up)
        );
        assert_eq!(
            direction_between(a, Coordinates::new(1, 0)),
            Some(Direction::Down)
        );
        assert_eq!(
            direction_between(a, Coordinates::new(2, 1)),
            Some(Direction::Right)
        );
        assert_eq!(
            direction_between(a, Coordinates::new(0, 1)),
            Some(Direction::Left)
        );
    }

    #[test]
    fn direction_between_non_adjacent() {
        let a = Coordinates::new(1, 1);
        assert_eq!(direction_between(a, Coordinates::new(3, 1)), None);
        assert_eq!(direction_between(a, Coordinates::new(2, 2)), None);
        assert_eq!(direction_between(a, a), None);
    }

    #[test]
    fn neighbors_center_open_grid() {
        let (mt, mud) = open_3x3();
        let center = Coordinates::new(1, 1);
        let mut n = neighbors(center, &mt, &mud);
        n.sort_by_key(|(c, _)| (c.x, c.y));

        assert_eq!(n.len(), 4);
        // All weights should be 1 (no mud)
        assert!(n.iter().all(|(_, w)| *w == 1));
    }

    #[test]
    fn neighbors_corner_open_grid() {
        let (mt, mud) = open_3x3();
        let corner = Coordinates::new(0, 0);
        let n = neighbors(corner, &mt, &mud);
        assert_eq!(n.len(), 2); // Up and Right only
    }

    #[test]
    fn neighbors_with_wall() {
        let (mt, mud) = walled_3x3();
        let pos = Coordinates::new(0, 1);
        let n = neighbors(pos, &mt, &mud);
        // (0,1) has: Up to (0,2), Down to (0,0), but Right to (1,1) is walled
        assert_eq!(n.len(), 2);
        let coords: Vec<_> = n.iter().map(|(c, _)| *c).collect();
        assert!(!coords.contains(&Coordinates::new(1, 1)));
    }

    #[test]
    fn neighbors_with_mud() {
        let (mt, mud) = muddy_3x3();
        let pos = Coordinates::new(1, 0);
        let n = neighbors(pos, &mt, &mud);
        // Should have neighbor (1,1) with weight 3
        let muddy_neighbor = n.iter().find(|(c, _)| *c == Coordinates::new(1, 1));
        assert_eq!(muddy_neighbor, Some(&(Coordinates::new(1, 1), 3)));
    }

    #[test]
    fn weight_open() {
        let (mt, mud) = open_3x3();
        assert_eq!(
            weight(Coordinates::new(0, 0), Coordinates::new(1, 0), &mt, &mud),
            Some(1)
        );
    }

    #[test]
    fn weight_walled() {
        let (mt, mud) = walled_3x3();
        assert_eq!(
            weight(Coordinates::new(0, 1), Coordinates::new(1, 1), &mt, &mud),
            None
        );
    }

    #[test]
    fn weight_mud() {
        let (mt, mud) = muddy_3x3();
        assert_eq!(
            weight(Coordinates::new(1, 0), Coordinates::new(1, 1), &mt, &mud),
            Some(3)
        );
    }

    #[test]
    fn weight_non_adjacent() {
        let (mt, mud) = open_3x3();
        assert_eq!(
            weight(Coordinates::new(0, 0), Coordinates::new(2, 2), &mt, &mud),
            None
        );
    }

    #[test]
    fn has_edge_open() {
        let (mt, _) = open_3x3();
        assert!(has_edge(
            Coordinates::new(0, 0),
            Coordinates::new(1, 0),
            &mt
        ));
        assert!(has_edge(
            Coordinates::new(1, 0),
            Coordinates::new(0, 0),
            &mt
        ));
    }

    #[test]
    fn has_edge_walled() {
        let (mt, _) = walled_3x3();
        assert!(!has_edge(
            Coordinates::new(0, 1),
            Coordinates::new(1, 1),
            &mt
        ));
        assert!(!has_edge(
            Coordinates::new(1, 1),
            Coordinates::new(0, 1),
            &mt
        ));
    }

    #[test]
    fn has_edge_non_adjacent() {
        let (mt, _) = open_3x3();
        assert!(!has_edge(
            Coordinates::new(0, 0),
            Coordinates::new(2, 2),
            &mt
        ));
    }

    #[test]
    fn effective_moves_center() {
        let (mt, _) = open_3x3();
        let moves = effective_moves(Coordinates::new(1, 1), &mt);
        assert_eq!(moves.len(), 4);
    }

    #[test]
    fn effective_moves_corner() {
        let (mt, _) = open_3x3();
        let moves = effective_moves(Coordinates::new(0, 0), &mt);
        assert_eq!(moves.len(), 2);
        assert!(moves.contains(&Direction::Up));
        assert!(moves.contains(&Direction::Right));
    }

    #[test]
    fn effective_moves_with_wall() {
        let (mt, _) = walled_3x3();
        let moves = effective_moves(Coordinates::new(0, 1), &mt);
        // Up to (0,2), Down to (0,0) — Right is walled, Left is boundary
        assert_eq!(moves.len(), 2);
        assert!(!moves.contains(&Direction::Right));
    }

    #[test]
    fn move_cost_open() {
        let (mt, mud) = open_3x3();
        assert_eq!(
            move_cost(Coordinates::new(1, 1), Direction::Up, &mt, &mud),
            Some(1)
        );
    }

    #[test]
    fn move_cost_wall() {
        let (mt, mud) = walled_3x3();
        assert_eq!(
            move_cost(Coordinates::new(0, 1), Direction::Right, &mt, &mud),
            None
        );
    }

    #[test]
    fn move_cost_boundary() {
        let (mt, mud) = open_3x3();
        assert_eq!(
            move_cost(Coordinates::new(0, 0), Direction::Down, &mt, &mud),
            None
        );
    }

    #[test]
    fn move_cost_mud() {
        let (mt, mud) = muddy_3x3();
        assert_eq!(
            move_cost(Coordinates::new(1, 0), Direction::Up, &mt, &mud),
            Some(3)
        );
    }
}
