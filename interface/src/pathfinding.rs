//! Dijkstra-based pathfinding on the maze graph.
//!
//! All functions take a [`Maze`] reference for topology.
//! Costs are in turns — mud passages cost N turns (N >= 2), free passages cost 1.

use crate::maze::{self, Maze};
use pyrat::{Coordinates, Direction};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// Result of a shortest-path query.
///
/// `cost` is the number of turns to reach `target` (mud passages cost N turns).
/// `first_moves` contains every direction that starts an optimal path — there
/// may be ties when multiple routes have the same cost.
#[derive(Debug, Clone, PartialEq)]
pub struct PathResult {
    pub target: Coordinates,
    pub first_moves: Vec<Direction>,
    pub cost: u32,
}

/// Shortest path between two cells. Returns `None` if unreachable.
///
/// When `from == to`, returns cost 0 with empty `first_moves`.
pub fn shortest_path(from: Coordinates, to: Coordinates, maze: &Maze) -> Option<PathResult> {
    if from == to {
        return Some(PathResult {
            target: to,
            first_moves: vec![],
            cost: 0,
        });
    }

    let size = maze.size();
    let mut dist = vec![u32::MAX; size];
    let mut first_moves: Vec<Vec<Direction>> = vec![vec![]; size];
    let mut heap: BinaryHeap<Reverse<(u32, Coordinates)>> = BinaryHeap::new();

    dist[from.to_index(maze.width())] = 0;
    heap.push(Reverse((0, from)));

    while let Some(Reverse((d, u))) = heap.pop() {
        let u_idx = u.to_index(maze.width());

        if d > dist[u_idx] {
            continue;
        }

        if u == to {
            return Some(PathResult {
                target: to,
                first_moves: first_moves[u_idx].clone(),
                cost: d,
            });
        }

        relax_neighbors(maze, from, u, d, &mut dist, &mut first_moves, &mut heap);
    }

    None
}

/// All cheeses at minimum distance from `from`, each with first-move options.
pub fn nearest_cheeses(from: Coordinates, cheese: &[Coordinates], maze: &Maze) -> Vec<PathResult> {
    if cheese.is_empty() {
        return vec![];
    }

    let size = maze.size();
    let mut dist = vec![u32::MAX; size];
    let mut first_moves: Vec<Vec<Direction>> = vec![vec![]; size];
    let mut heap: BinaryHeap<Reverse<(u32, Coordinates)>> = BinaryHeap::new();

    dist[from.to_index(maze.width())] = 0;
    heap.push(Reverse((0, from)));

    let mut min_cheese_dist: Option<u32> = None;

    while let Some(Reverse((d, u))) = heap.pop() {
        let u_idx = u.to_index(maze.width());

        if d > dist[u_idx] {
            continue;
        }

        // Once we've popped past the minimum cheese distance, all remaining
        // cheese at that distance are already settled — stop.
        if let Some(min_d) = min_cheese_dist {
            if d > min_d {
                break;
            }
        }

        if cheese.contains(&u) && min_cheese_dist.is_none() {
            min_cheese_dist = Some(d);
        }

        relax_neighbors(maze, from, u, d, &mut dist, &mut first_moves, &mut heap);
    }

    let Some(min_d) = min_cheese_dist else {
        return vec![];
    };

    cheese
        .iter()
        .filter_map(|&c| {
            let idx = c.to_index(maze.width());
            if dist[idx] == min_d {
                Some(PathResult {
                    target: c,
                    first_moves: first_moves[idx].clone(),
                    cost: min_d,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Weighted distances from `pos` to all reachable cells.
pub fn distances_from(pos: Coordinates, maze: &Maze) -> HashMap<Coordinates, u32> {
    let size = maze.size();
    let mut dist = vec![u32::MAX; size];
    let mut heap: BinaryHeap<Reverse<(u32, Coordinates)>> = BinaryHeap::new();

    dist[pos.to_index(maze.width())] = 0;
    heap.push(Reverse((0, pos)));

    while let Some(Reverse((d, u))) = heap.pop() {
        let u_idx = u.to_index(maze.width());

        if d > dist[u_idx] {
            continue;
        }

        for (neighbor, w) in maze.neighbors(u) {
            let new_dist = d + w as u32;
            let n_idx = neighbor.to_index(maze.width());

            if new_dist < dist[n_idx] {
                dist[n_idx] = new_dist;
                heap.push(Reverse((new_dist, neighbor)));
            }
        }
    }

    let mut result = HashMap::new();
    for y in 0..maze.height() {
        for x in 0..maze.width() {
            let c = Coordinates::new(x, y);
            let idx = c.to_index(maze.width());
            if dist[idx] != u32::MAX {
                result.insert(c, dist[idx]);
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Relax all neighbors of `u` in the Dijkstra search, tracking first-move provenance.
fn relax_neighbors(
    maze: &Maze,
    source: Coordinates,
    u: Coordinates,
    d: u32,
    dist: &mut [u32],
    first_moves: &mut [Vec<Direction>],
    heap: &mut BinaryHeap<Reverse<(u32, Coordinates)>>,
) {
    let u_idx = u.to_index(maze.width());

    for (neighbor, w) in maze.neighbors(u) {
        let new_dist = d + w as u32;
        let n_idx = neighbor.to_index(maze.width());

        if new_dist < dist[n_idx] {
            dist[n_idx] = new_dist;
            first_moves[n_idx] = if u == source {
                vec![maze::direction_between(source, neighbor).unwrap()]
            } else {
                first_moves[u_idx].clone()
            };
            heap.push(Reverse((new_dist, neighbor)));
        } else if new_dist == dist[n_idx] {
            let moves_to_merge = if u == source {
                vec![maze::direction_between(source, neighbor).unwrap()]
            } else {
                first_moves[u_idx].clone()
            };
            for m in moves_to_merge {
                if !first_moves[n_idx].contains(&m) {
                    first_moves[n_idx].push(m);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat::{GameBuilder, MoveTable, MudMap};
    use std::collections::HashMap;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn open_grid(w: u8, h: u8) -> (MoveTable, MudMap, u8, u8) {
        let game = GameBuilder::new(w, h)
            .with_custom_maze(HashMap::new(), MudMap::new())
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(w - 1, h - 1))
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build()
            .create(None)
            .unwrap();
        (game.move_table, game.mud, w, h)
    }

    fn grid_with_walls(
        w: u8,
        h: u8,
        wall_pairs: &[(Coordinates, Coordinates)],
    ) -> (MoveTable, MudMap, u8, u8) {
        let mut walls: HashMap<Coordinates, Vec<Coordinates>> = HashMap::new();
        for &(a, b) in wall_pairs {
            walls.entry(a).or_default().push(b);
            walls.entry(b).or_default().push(a);
        }
        let game = GameBuilder::new(w, h)
            .with_custom_maze(walls, MudMap::new())
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(w - 1, h - 1))
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build()
            .create(None)
            .unwrap();
        (game.move_table, game.mud, w, h)
    }

    fn grid_with_mud(
        w: u8,
        h: u8,
        mud_entries: &[(Coordinates, Coordinates, u8)],
    ) -> (MoveTable, MudMap, u8, u8) {
        let mut mud = MudMap::new();
        for &(a, b, v) in mud_entries {
            mud.insert(a, b, v);
        }
        let game = GameBuilder::new(w, h)
            .with_custom_maze(HashMap::new(), mud)
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(w - 1, h - 1))
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build()
            .create(None)
            .unwrap();
        (game.move_table, game.mud, w, h)
    }

    fn grid_with_walls_and_mud(
        w: u8,
        h: u8,
        wall_pairs: &[(Coordinates, Coordinates)],
        mud_entries: &[(Coordinates, Coordinates, u8)],
    ) -> (MoveTable, MudMap, u8, u8) {
        let mut walls: HashMap<Coordinates, Vec<Coordinates>> = HashMap::new();
        for &(a, b) in wall_pairs {
            walls.entry(a).or_default().push(b);
            walls.entry(b).or_default().push(a);
        }
        let mut mud = MudMap::new();
        for &(a, b, v) in mud_entries {
            mud.insert(a, b, v);
        }
        let game = GameBuilder::new(w, h)
            .with_custom_maze(walls, mud)
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(w - 1, h - 1))
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build()
            .create(None)
            .unwrap();
        (game.move_table, game.mud, w, h)
    }

    fn c(x: u8, y: u8) -> Coordinates {
        Coordinates::new(x, y)
    }

    // -----------------------------------------------------------------------
    // shortest_path tests
    // -----------------------------------------------------------------------

    #[test]
    fn same_position() {
        let (mt, mud, w, h) = open_grid(3, 3);
        let maze = Maze::new(&mt, &mud, w, h);
        let result = shortest_path(c(1, 1), c(1, 1), &maze).unwrap();
        assert_eq!(result.cost, 0);
        assert!(result.first_moves.is_empty());
    }

    #[test]
    fn adjacent_open() {
        let (mt, mud, w, h) = open_grid(3, 3);
        let maze = Maze::new(&mt, &mud, w, h);
        let result = shortest_path(c(0, 0), c(1, 0), &maze).unwrap();
        assert_eq!(result.cost, 1);
        assert_eq!(result.first_moves, vec![Direction::Right]);
    }

    #[test]
    fn open_grid_manhattan() {
        let (mt, mud, w, h) = open_grid(5, 5);
        let maze = Maze::new(&mt, &mud, w, h);
        let result = shortest_path(c(0, 0), c(4, 4), &maze).unwrap();
        assert_eq!(result.cost, 8);
    }

    #[test]
    fn wall_forces_detour() {
        // Wall between (1,0) and (1,1). Going from (1,0) to (1,1) must detour.
        let (mt, mud, w, h) = grid_with_walls(3, 3, &[(c(1, 0), c(1, 1))]);
        let maze = Maze::new(&mt, &mud, w, h);
        let result = shortest_path(c(1, 0), c(1, 1), &maze).unwrap();
        // Detour: (1,0)→(0,0)→(0,1)→(1,1) or (1,0)→(2,0)→(2,1)→(1,1) = cost 3
        assert_eq!(result.cost, 3);
        // Both detours start with Left or Right
        let mut moves = result.first_moves.clone();
        moves.sort_by_key(|d| *d as u8);
        assert_eq!(moves, vec![Direction::Right, Direction::Left]);
    }

    #[test]
    fn mud_adds_cost() {
        // Mud=3 between (0,0) and (0,1)
        let (mt, mud, w, h) = grid_with_mud(3, 3, &[(c(0, 0), c(0, 1), 3)]);
        let maze = Maze::new(&mt, &mud, w, h);
        // Direct (0,0)→(0,1) = 3. Alt: (0,0)→(1,0)→(1,1)→(0,1) = 3 also.
        let result = shortest_path(c(0, 0), c(0, 1), &maze).unwrap();
        assert_eq!(result.cost, 3);
    }

    #[test]
    fn heavy_mud_avoided() {
        // Mud=5 between (0,0) and (1,0).
        // Direct = 5. Around via (0,1)→(1,1)→(1,0) = 3.
        let (mt, mud, w, h) = grid_with_mud(3, 3, &[(c(0, 0), c(1, 0), 5)]);
        let maze = Maze::new(&mt, &mud, w, h);
        let result = shortest_path(c(0, 0), c(1, 0), &maze).unwrap();
        assert_eq!(result.cost, 3);
        assert_eq!(result.first_moves, vec![Direction::Up]);
    }

    #[test]
    fn unreachable_returns_none() {
        // Wall off (2,2) completely: walls between (1,2)-(2,2) and (2,1)-(2,2)
        let (mt, mud, w, h) = grid_with_walls(3, 3, &[(c(1, 2), c(2, 2)), (c(2, 1), c(2, 2))]);
        let maze = Maze::new(&mt, &mud, w, h);
        let result = shortest_path(c(0, 0), c(2, 2), &maze);
        assert!(result.is_none());
    }

    #[test]
    fn tie_breaking_multiple_first_moves() {
        // (0,0) to (1,1) on open grid: Right+Up or Up+Right, both cost 2
        let (mt, mud, w, h) = open_grid(3, 3);
        let maze = Maze::new(&mt, &mud, w, h);
        let result = shortest_path(c(0, 0), c(1, 1), &maze).unwrap();
        assert_eq!(result.cost, 2);
        let mut moves = result.first_moves.clone();
        moves.sort_by_key(|d| *d as u8);
        assert_eq!(moves, vec![Direction::Up, Direction::Right]);
    }

    #[test]
    fn forced_through_mud_exact_cost() {
        // 5x2 grid. Walls between y=0 and y=1 at x=1,2,3.
        // Mud on (1,0)-(2,0)=3 and (3,0)-(4,0)=2.
        // Bottom path: 1 + 3 + 1 + 2 = 7.
        // Top path: (0,0)→(0,1)→(1,1)→(2,1)→(3,1)→(4,1)→(4,0) = 6.
        let (mt, mud, w, h) = grid_with_walls_and_mud(
            5,
            2,
            &[(c(1, 0), c(1, 1)), (c(2, 0), c(2, 1)), (c(3, 0), c(3, 1))],
            &[(c(1, 0), c(2, 0), 3), (c(3, 0), c(4, 0), 2)],
        );
        let maze = Maze::new(&mt, &mud, w, h);
        let result = shortest_path(c(0, 0), c(4, 0), &maze).unwrap();
        assert_eq!(result.cost, 6);
    }

    #[test]
    fn all_mud_pick_cheapest() {
        // 2x2 grid. Mud=5 on (0,0)-(1,0), mud=2 on (0,0)-(0,1).
        // Direct: 5. Around: 2 + 1 + 1 = 4.
        let (mt, mud, w, h) = grid_with_mud(2, 2, &[(c(0, 0), c(1, 0), 5), (c(0, 0), c(0, 1), 2)]);
        let maze = Maze::new(&mt, &mud, w, h);
        let result = shortest_path(c(0, 0), c(1, 0), &maze).unwrap();
        assert_eq!(result.cost, 4);
        assert_eq!(result.first_moves, vec![Direction::Up]);
    }

    // -----------------------------------------------------------------------
    // nearest_cheeses tests
    // -----------------------------------------------------------------------

    #[test]
    fn nearest_cheese_simple() {
        let (mt, mud, w, h) = open_grid(5, 5);
        let maze = Maze::new(&mt, &mud, w, h);
        let cheese = vec![c(1, 0), c(4, 4)];
        let results = nearest_cheeses(c(0, 0), &cheese, &maze);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].target, c(1, 0));
        assert_eq!(results[0].cost, 1);
    }

    #[test]
    fn nearest_cheese_equidistant() {
        let (mt, mud, w, h) = open_grid(5, 5);
        let maze = Maze::new(&mt, &mud, w, h);
        let cheese = vec![c(1, 2), c(3, 2)];
        let results = nearest_cheeses(c(2, 2), &cheese, &maze);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.cost == 1));
    }

    #[test]
    fn nearest_cheese_empty() {
        let (mt, mud, w, h) = open_grid(3, 3);
        let maze = Maze::new(&mt, &mud, w, h);
        let results = nearest_cheeses(c(0, 0), &[], &maze);
        assert!(results.is_empty());
    }

    #[test]
    fn nearest_cheese_on_position() {
        let (mt, mud, w, h) = open_grid(3, 3);
        let maze = Maze::new(&mt, &mud, w, h);
        let cheese = vec![c(1, 1), c(2, 2)];
        let results = nearest_cheeses(c(1, 1), &cheese, &maze);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].target, c(1, 1));
        assert_eq!(results[0].cost, 0);
        assert!(results[0].first_moves.is_empty());
    }

    // -----------------------------------------------------------------------
    // distances_from tests
    // -----------------------------------------------------------------------

    #[test]
    fn distances_open_grid_manhattan() {
        let (mt, mud, w, h) = open_grid(3, 3);
        let maze = Maze::new(&mt, &mud, w, h);
        let dists = distances_from(c(0, 0), &maze);

        assert_eq!(dists[&c(0, 0)], 0);
        assert_eq!(dists[&c(1, 0)], 1);
        assert_eq!(dists[&c(0, 1)], 1);
        assert_eq!(dists[&c(1, 1)], 2);
        assert_eq!(dists[&c(2, 2)], 4);
        assert_eq!(dists.len(), 9);
    }

    #[test]
    fn distances_with_mud() {
        let (mt, mud, w, h) = grid_with_mud(3, 3, &[(c(0, 0), c(1, 0), 3)]);
        let maze = Maze::new(&mt, &mud, w, h);
        let dists = distances_from(c(0, 0), &maze);
        // Direct (0,0)→(1,0) = 3 via mud. Around = 3 also. Either way, cost 3.
        assert_eq!(dists[&c(1, 0)], 3);
    }

    #[test]
    fn distances_unreachable_cell() {
        let (mt, mud, w, h) = grid_with_walls(3, 3, &[(c(1, 2), c(2, 2)), (c(2, 1), c(2, 2))]);
        let maze = Maze::new(&mt, &mud, w, h);
        let dists = distances_from(c(0, 0), &maze);
        assert!(!dists.contains_key(&c(2, 2)));
        assert_eq!(dists.len(), 8);
    }
}
