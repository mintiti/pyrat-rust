use std::convert::TryFrom;

use itertools::Itertools;
use rand::seq::SliceRandom;

#[derive(Debug)]
pub struct Player<'node> {
    pub node: &'node Node,
    pub score: f64,
    pub stuck: u8,
}

#[derive(Debug)]
pub struct Board<'node> {
    pub players: Vec<Player<'node>>,
    pub maze: Maze,
}

#[derive(Debug)]
struct NeighbourData {
    index: u8,
    distance: u8,
    // 16 bits
}

#[derive(Debug)]
struct Node {
    neighbour_up: NeighbourData,
    neighbour_right: NeighbourData,
    neighbour_down: NeighbourData,
    neighbour_left: NeighbourData,
    // 16*4 = 64 bits
}

#[derive(Debug)]
struct Maze {
    nodes: Vec<Node>,
    pub(crate) cheeses: Vec<u8>,
    pub(crate) max_x: u8,
    pub(crate) max_y: u8,
}

impl Maze {
    fn fully_connected(max_x: u8, max_y: u8, nb_cheese: u32) -> Maze {
        let nb_tiles: usize = max_x as usize * max_y as usize;
        if nb_tiles < nb_cheese as usize {
            panic!("Number of cheeses {} is greater than the number of tiles in the maze {} !", nb_cheese, nb_tiles)
        }
        let mut maze: Maze = Maze {
            nodes: Vec::with_capacity(nb_tiles),
            cheeses: Vec::with_capacity(nb_tiles),
            max_x,
            max_y,
        };
        let mut nb_cheeses_remaining: u32 = nb_cheese;
        let mut available_tiles: Vec<(u8, u8)> = (0..max_x).cartesian_product(0..max_y).collect();
        // Initialize the nodes and cheeses
        // The nodes are originally only linked to themselves.
        for i in 0..nb_tiles {
            maze.nodes[i] = Node {
                neighbour_up: NeighbourData {
                    index: i.try_into().unwrap(),
                    distance: 1,
                },
                neighbour_left: NeighbourData {
                    index: i.try_into().unwrap(),
                    distance: 1,
                },
                neighbour_right: NeighbourData {
                    index: i.try_into().unwrap(),
                    distance: 1,
                },
                neighbour_down: NeighbourData {
                    index: i.try_into().unwrap(),
                    distance: 1,
                },
            };
            maze.cheeses[i] = 0;
        }

        if nb_cheese % 2 == 1 {
            // Place one cheese in the center of the maze
            if (max_x % 2 != 1) | (max_y % 2 != 1) {
                panic!("The number cheeses is uneven, and one of the dimensions of the maze is even. We cannot create a symmetric maze")
            } else {
                // put one cheese at the center of everything
                maze.cheeses[nb_tiles / 2] = 1;
                available_tiles.remove(available_tiles.iter().position(|tile| (tile.0 == max_x / 2) && (tile.1 == max_y / 2)).expect("Tile not found"));
                nb_cheeses_remaining -= 1;
            }
        }
        let middle_x: u8 = max_x / 2;
        let middle_y: u8 = max_y / 2;
        while nb_cheeses_remaining > 0 {
            let tile: &(u8, u8) = available_tiles.choose(&mut rand::thread_rng()).unwrap();
            let symmetric_tile: (u8, u8) = (2 * middle_x - tile.0, 2 * middle_y - tile.1);
            maze.cheeses[to_node_index(tile.0.into(), tile.1.into(), &maze).unwrap()] = 1;
            maze.cheeses[to_node_index(symmetric_tile.0.into(), tile.1.into(), &maze).unwrap()] = 1;
            nb_cheeses_remaining -= 2;
            available_tiles.remove(available_tiles.iter().position(|element| (element.0 == tile.0) && (element.1 == tile.1)).expect("Tile not found"));
            available_tiles.remove(available_tiles.iter().position(|element| (element.0 == symmetric_tile.0) && (element.1 == symmetric_tile.1)).expect("Tile not found"));
        }

        // Fully connect the neighbours
        for i in 0..maze.nodes.len() {
            maybe_add_up(i, &mut maze);
            maybe_add_right(i, &mut maze);
            maybe_add_down(i, &mut maze);
            maybe_add_left(i, &mut maze);
        }
        return maze;
    }
}

fn maybe_add_up(index: usize, maze: &mut Maze) {
    if let Some(neighbour_index) = index.checked_add(maze.max_x.into()) {
        if neighbour_index < maze.nodes.len() {
            maze.nodes[index].neighbour_up.index = u8::try_from(neighbour_index).unwrap();
        }
    } else {
        panic!("Overflow occured")
    }
}

fn maybe_add_right(index: usize, maze: &mut Maze) {
    if let Some(neighbour_index) = index.checked_add(1) {
        if (neighbour_index < maze.nodes.len()) && (neighbour_index % maze.max_x as usize != 0) {
            maze.nodes[index].neighbour_right.index = u8::try_from(neighbour_index).unwrap();
        }
    } else {
        panic!("Overflow occured")
    }
}

fn maybe_add_down(index: usize, maze: &mut Maze) {
    if let Some(neighbour_index) = index.checked_add_signed(-(maze.max_x as isize)) {
        if neighbour_index >= 0 {
            maze.nodes[index].neighbour_down.index = u8::try_from(neighbour_index).unwrap();
        }
    } else {
        panic!("Overflow occured")
    }
}

fn maybe_add_left(index: usize, maze: &mut Maze) {
    if let Some(neighbour_index) = index.checked_add_signed(-1) {
        if (neighbour_index >= 0) && (index % maze.max_x as usize != 0) {
            maze.nodes[index].neighbour_left.index = u8::try_from(neighbour_index).unwrap();
        }
    }
}

pub fn to_node_index(x: u32, y: u32, board: &Maze) -> Option<usize> {
    if (x > (board.max_x.into())) | (y > (board.max_y.into())) {
        return None;
    }
    return Some((x as usize + y as usize * (board.max_x as usize)).into());
}

pub enum Move {
    Up,
    Right,
    Down,
    Left,
}


impl<'node> Board<'node> {
    pub fn new(max_x: u8, max_y: u8, nb_cheese: u32) -> Self {
        // Initialize a new board
        let mut board: Board = Board {
            players: Vec::with_capacity(2),
            maze: Maze::fully_connected(max_x, max_y, nb_cheese),
        };
        board.players.push(Player {
            node: &board.maze.nodes[0],
            score: 0.0,
            stuck: 0,
        });
        board.players.push(Player {
            node: &board.maze.nodes.last().unwrap(),
            score: 0.0,
            stuck: 0,
        });
        return board;
    }
}





