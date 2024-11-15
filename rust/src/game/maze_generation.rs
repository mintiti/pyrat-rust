use crate::Coordinates;
use rand::prelude::SliceRandom;
use rand::Rng;
use std::collections::{HashMap, HashSet};

pub type WallMap = HashMap<Coordinates, Vec<Coordinates>>;
pub type MudMap = HashMap<(Coordinates, Coordinates), u8>;

/// Configuration for maze generation
#[derive(Debug, Clone, Copy)]
pub struct MazeConfig {
    pub width: u8,
    pub height: u8,
    pub target_density: f32, // Probability of not having a wall (0.0 to 1.0)
    pub connected: bool,     // Whether the maze must be fully connected
    pub symmetry: bool,      // Whether the maze should be symmetric
    pub mud_density: f32,    // Probability of mud in valid passages (0.0 to 1.0)
    pub mud_range: u8,       // Maximum mud value (minimum is 2)
    pub seed: Option<u64>,   // Optional seed for reproducibility
}

/// Generates a complete maze with all components
pub struct MazeGenerator {
    config: MazeConfig,
    rng: rand::rngs::StdRng,
    walls: HashMap<Coordinates, Vec<Coordinates>>,
    mud: HashMap<(Coordinates, Coordinates), u8>,
}

impl MazeGenerator {
    /// Creates a new maze generator with the given configuration
    #[must_use]
    pub fn new(config: MazeConfig) -> Self {
        let rng = config
            .seed
            .map_or_else(rand::SeedableRng::from_entropy, |seed| {
                rand::SeedableRng::seed_from_u64(seed)
            });

        Self {
            config,
            rng,
            walls: HashMap::new(),
            mud: HashMap::new(),
        }
    }

    /// Generates a complete maze with walls and mud
    pub fn generate(&mut self) -> (WallMap, MudMap) {
        self.generate_initial_layout();

        if self.config.connected {
            self.ensure_connectivity();
        }

        self.add_border_connections();

        (self.walls.clone(), self.mud.clone())
    }

    /// Generates the initial random layout of the maze
    fn generate_initial_layout(&mut self) {
        let mut not_considered = HashSet::new();

        // Initialize maze and not_considered exactly as in Python
        for x in 0..self.config.width {
            for y in 0..self.config.height {
                not_considered.insert(Coordinates::new(x, y));
            }
        }

        // Generate passages following Python logic exactly
        for i in 0..self.config.width {
            for j in 0..self.config.height {
                let current = Coordinates::new(i, j);

                if !self.config.symmetry || not_considered.contains(&current) {
                    // Horizontal connections (exactly as Python)
                    if i + 1 < self.config.width
                        && self.rng.gen::<f32>() > self.config.target_density
                    {
                        let next = Coordinates::new(i + 1, j);
                        let mud_value = if self.rng.gen::<f32>() < self.config.mud_density {
                            self.rng.gen_range(2..=self.config.mud_range)
                        } else {
                            1 // Python uses 1 for no mud
                        };

                        // Add bidirectional connection
                        self.walls.entry(current).or_default().push(next);
                        self.walls.entry(next).or_default().push(current);

                        if mud_value > 1 {
                            self.mud.insert((current, next), mud_value);
                            self.mud.insert((next, current), mud_value);
                        }

                        // Handle symmetry exactly as Python
                        if self.config.symmetry {
                            let sym_current = self.get_symmetric(current);
                            let sym_next = self.get_symmetric(next);

                            self.walls.entry(sym_current).or_default().push(sym_next);
                            self.walls.entry(sym_next).or_default().push(sym_current);

                            if mud_value > 1 {
                                self.mud.insert((sym_current, sym_next), mud_value);
                                self.mud.insert((sym_next, sym_current), mud_value);
                            }
                        }
                    }

                    // Vertical connections (exactly as Python)
                    if j + 1 < self.config.height
                        && self.rng.gen::<f32>() > self.config.target_density
                    {
                        let next = Coordinates::new(i, j + 1);
                        let mud_value = if self.rng.gen::<f32>() < self.config.mud_density {
                            self.rng.gen_range(2..=self.config.mud_range)
                        } else {
                            1
                        };

                        self.walls.entry(current).or_default().push(next);
                        self.walls.entry(next).or_default().push(current);

                        if mud_value > 1 {
                            self.mud.insert((current, next), mud_value);
                            self.mud.insert((next, current), mud_value);
                        }

                        if self.config.symmetry {
                            let sym_current = self.get_symmetric(current);
                            let sym_next = self.get_symmetric(next);

                            self.walls.entry(sym_current).or_default().push(sym_next);
                            self.walls.entry(sym_next).or_default().push(sym_current);

                            if mud_value > 1 {
                                self.mud.insert((sym_current, sym_next), mud_value);
                                self.mud.insert((sym_next, sym_current), mud_value);
                            }
                        }
                    }

                    if self.config.symmetry {
                        not_considered.remove(&current);
                        not_considered.remove(&self.get_symmetric(current));
                    }
                }
            }
        }
    }

    /// Ensures the maze is fully connected using a modified DFS algorithm
    fn ensure_connectivity(&mut self) {
        let mut connected =
            vec![vec![false; self.config.height as usize]; self.config.width as usize];
        let mut possible_border = Vec::new();

        // Start from top-left corner
        let start = Coordinates::new(0, self.config.height - 1);
        connected[0][self.config.height as usize - 1] = true;
        possible_border.push(start);

        self.connect_region(&mut connected, &mut possible_border);
    }

    /// Recursively connects regions of the maze using DFS
    fn connect_region(
        &mut self,
        connected: &mut [Vec<bool>],
        possible_border: &mut Vec<Coordinates>,
    ) {
        while !possible_border.is_empty() {
            let mut border = Vec::new();
            let mut new_possible_border = Vec::new();

            // Match Python's border creation exactly
            for &current in possible_border.iter() {
                let mut is_candidate = false;
                let x = current.x as usize;
                let y = current.y as usize;

                // Check each direction exactly as Python does
                if current.x + 1 < self.config.width
                    && !self.has_connection(current, Coordinates::new(current.x + 1, current.y))
                    && !connected[(current.x + 1) as usize][y]
                {
                    border.push((current, Coordinates::new(current.x + 1, current.y)));
                    is_candidate = true;
                }
                if current.x > 0
                    && !self.has_connection(current, Coordinates::new(current.x - 1, current.y))
                    && !connected[(current.x - 1) as usize][y]
                {
                    border.push((current, Coordinates::new(current.x - 1, current.y)));
                    is_candidate = true;
                }
                if current.y + 1 < self.config.height
                    && !self.has_connection(current, Coordinates::new(current.x, current.y + 1))
                    && !connected[x][(current.y + 1) as usize]
                {
                    border.push((current, Coordinates::new(current.x, current.y + 1)));
                    is_candidate = true;
                }
                if current.y > 0
                    && !self.has_connection(current, Coordinates::new(current.x, current.y - 1))
                    && !connected[x][(current.y - 1) as usize]
                {
                    border.push((current, Coordinates::new(current.x, current.y - 1)));
                    is_candidate = true;
                }

                if is_candidate {
                    new_possible_border.push(current);
                }
            }

            if border.is_empty() {
                break;
            }

            // Select random border exactly as Python
            let idx = self.rng.gen_range(0..border.len());
            let (from, to) = border[idx];

            // Generate mud exactly as Python
            let mud_value = if self.rng.gen::<f32>() < self.config.mud_density {
                self.rng.gen_range(2..=self.config.mud_range)
            } else {
                1
            };

            // Add connections
            self.walls.entry(from).or_default().push(to);
            self.walls.entry(to).or_default().push(from);

            if mud_value > 1 {
                self.mud.insert((from, to), mud_value);
                self.mud.insert((to, from), mud_value);
            }

            // Handle symmetry exactly as Python
            if self.config.symmetry {
                let sym_from = self.get_symmetric(from);
                let sym_to = self.get_symmetric(to);

                self.walls.entry(sym_from).or_default().push(sym_to);
                self.walls.entry(sym_to).or_default().push(sym_from);

                if mud_value > 1 {
                    self.mud.insert((sym_from, sym_to), mud_value);
                    self.mud.insert((sym_to, sym_from), mud_value);
                }
            }

            connected[to.x as usize][to.y as usize] = true;
            possible_border.push(to);
            *possible_border = new_possible_border;
        }
    }
    #[inline]
    fn has_connection(&self, from: Coordinates, to: Coordinates) -> bool {
        self.walls
            .get(&from)
            .map_or(false, |connections| connections.contains(&to))
    }

    /// Adds border connections to ensure no isolated cells
    fn add_border_connections(&mut self) {
        for x in 0..self.config.width {
            for y in 0..self.config.height {
                let current = Coordinates::new(x, y);
                if self.is_border_cell(current) && !self.has_any_connection(current) {
                    let neighbors = self.get_valid_neighbors(current);
                    if let Some(&neighbor) = neighbors.choose(&mut self.rng) {
                        self.add_passage(current, neighbor);

                        if self.config.symmetry {
                            let sym_current = self.get_symmetric(current);
                            let sym_neighbor = self.get_symmetric(neighbor);
                            self.add_passage(sym_current, sym_neighbor);
                        }
                    }
                }
            }
        }
    }

    /// Adds a passage between two cells with optional mud
    #[inline(always)]
    fn add_passage(&mut self, from: Coordinates, to: Coordinates) {
        // First add the walls bidirectionally
        self.walls.entry(from).or_default().push(to);
        self.walls.entry(to).or_default().push(from);

        // Then handle mud if needed
        if self.rng.gen::<f32>() < self.config.mud_density {
            let mud_value = self.rng.gen_range(2..=self.config.mud_range);

            // Add mud both ways for the passage
            self.mud.insert((from, to), mud_value);
            self.mud.insert((to, from), mud_value);

            // If symmetric, add mud for the symmetric passage
            if self.config.symmetry {
                let sym_from = self.get_symmetric(from);
                let sym_to = self.get_symmetric(to);
                self.mud.insert((sym_from, sym_to), mud_value);
                self.mud.insert((sym_to, sym_from), mud_value);
            }
        }
    }

    /// Gets the symmetric position for a given coordinate
    #[inline(always)]
    const fn get_symmetric(&self, pos: Coordinates) -> Coordinates {
        Coordinates::new(
            self.config.width - 1 - pos.x,
            self.config.height - 1 - pos.y,
        )
    }

    /// Checks if a cell is on the border of the maze
    #[inline(always)]
    const fn is_border_cell(&self, pos: Coordinates) -> bool {
        pos.x == 0
            || pos.y == 0
            || pos.x == self.config.width - 1
            || pos.y == self.config.height - 1
    }

    /// Gets all valid neighboring cells
    fn get_valid_neighbors(&self, pos: Coordinates) -> Vec<Coordinates> {
        let mut neighbors = Vec::new();
        let directions = [(0, 1), (1, 0), (0, -1), (-1, 0)];

        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        for (dx, dy) in &directions {
            let new_x = i32::from(pos.x) + dx;
            let new_y = i32::from(pos.y) + dy;

            if new_x >= 0
                && new_x < i32::from(self.config.width)
                && new_y >= 0
                && new_y < i32::from(self.config.height)
            {
                neighbors.push(Coordinates::new(new_x as u8, new_y as u8));
            }
        }

        neighbors
    }

    /// Checks if a cell has any connections
    #[inline(always)]
    fn has_any_connection(&self, pos: Coordinates) -> bool {
        self.walls
            .get(&pos)
            .map_or(false, |connections| !connections.is_empty())
    }
}

/// Cheese placement configuration
#[derive(Debug, Clone)]
pub struct CheeseConfig {
    pub count: u16,     // Number of cheese pieces to place
    pub symmetry: bool, // Whether cheese placement should be symmetric
}

pub struct CheeseGenerator {
    config: CheeseConfig,
    rng: rand::rngs::StdRng,
    width: u8,
    height: u8,
}

impl CheeseGenerator {
    #[must_use]
    pub fn new(config: CheeseConfig, width: u8, height: u8, seed: Option<u64>) -> Self {
        let rng = seed.map_or_else(rand::SeedableRng::from_entropy, |seed| {
            rand::SeedableRng::seed_from_u64(seed)
        });

        Self {
            config,
            rng,
            width,
            height,
        }
    }

    /// Generate cheese placements.
    ///
    /// # Panics
    /// - When attempting to place odd number of cheese in symmetric maze with even dimensions
    /// - When requesting more cheese pieces than available positions in the maze
    pub fn generate(
        &mut self,
        player1_pos: Coordinates,
        player2_pos: Coordinates,
    ) -> Vec<Coordinates> {
        let mut pieces = Vec::new();
        let mut remaining = self.config.count;

        // Handle center piece for odd counts in symmetric mazes
        if self.config.symmetry && remaining % 2 == 1 {
            assert!(
                !(self.width % 2 == 0 || self.height % 2 == 0),
                "Cannot place odd number of cheese in symmetric maze with even dimensions"
            );
            let center = Coordinates::new(self.width / 2, self.height / 2);
            if center != player1_pos && center != player2_pos {
                pieces.push(center);
                remaining -= 1;
            }
        }

        // Generate candidate positions
        let mut candidates = Vec::new();
        let mut considered = HashSet::new();

        for x in 0..self.width {
            for y in 0..self.height {
                let pos = Coordinates::new(x, y);
                if (!self.config.symmetry || !considered.contains(&pos))
                    && pos != player1_pos
                    && pos != player2_pos
                    && pos != self.get_symmetric(pos)
                {
                    candidates.push(pos);
                    if self.config.symmetry {
                        considered.insert(pos);
                        considered.insert(self.get_symmetric(pos));
                    }
                }
            }
        }

        // Place remaining pieces
        while remaining > 0 && !candidates.is_empty() {
            let idx = self.rng.gen_range(0..candidates.len());
            let chosen = candidates.swap_remove(idx);
            pieces.push(chosen);

            if self.config.symmetry {
                let symmetric = self.get_symmetric(chosen);
                pieces.push(symmetric);
                candidates.retain(|&pos| pos != symmetric);
                remaining -= 2;
            } else {
                remaining -= 1;
            }
        }

        assert!(
            remaining == 0,
            "Too many pieces of cheese for maze dimensions"
        );

        pieces
    }

    /// Gets the symmetric position for a given coordinate
    #[inline(always)]
    const fn get_symmetric(&self, pos: Coordinates) -> Coordinates {
        Coordinates::new(self.width - 1 - pos.x, self.height - 1 - pos.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_maze_generation() {
        let config = MazeConfig {
            width: 10,
            height: 10,
            target_density: 0.7,
            connected: true,
            symmetry: false,
            mud_density: 0.2,
            mud_range: 3,
            seed: Some(42),
        };

        let mut generator = MazeGenerator::new(config);
        let (walls, mud) = generator.generate();

        // Check basic properties
        assert!(!walls.is_empty());
        assert!(mud.len() <= walls.len());
    }

    #[test]
    fn test_symmetric_maze_generation() {
        let config = MazeConfig {
            width: 11, // Odd dimensions for symmetry
            height: 11,
            target_density: 0.7,
            connected: true,
            symmetry: true,
            mud_density: 0.2,
            mud_range: 3,
            seed: Some(42),
        };

        let mut generator = MazeGenerator::new(config.clone());
        let (walls, mud) = generator.generate();

        // Check symmetry
        for (from, connections) in walls.iter() {
            let sym_from = Coordinates::new(config.width - 1 - from.x, config.height - 1 - from.y);
            let sym_connections = walls.get(&sym_from).unwrap();

            // Check that symmetric connections exist
            for to in connections {
                let sym_to = Coordinates::new(config.width - 1 - to.x, config.height - 1 - to.y);
                assert!(sym_connections.contains(&sym_to));
            }
        }

        // Check mud symmetry
        for ((from, to), value) in mud.iter() {
            let sym_from = Coordinates::new(config.width - 1 - from.x, config.height - 1 - from.y);
            let sym_to = Coordinates::new(config.width - 1 - to.x, config.height - 1 - to.y);
            assert_eq!(mud.get(&(sym_from, sym_to)), Some(value));
        }
    }

    #[test]
    fn test_maze_connectivity() {
        let config = MazeConfig {
            width: 8,
            height: 8,
            target_density: 0.3, // Lower density means more connections
            connected: true,
            symmetry: false,
            mud_density: 0.2,
            mud_range: 3,
            seed: Some(42),
        };

        let mut generator = MazeGenerator::new(config.clone());
        let (walls, _) = generator.generate();

        // Check if all cells are reachable from starting position
        let mut visited = HashSet::new();
        let mut stack = vec![Coordinates::new(0, 0)];

        while let Some(current) = stack.pop() {
            if visited.insert(current) {
                if let Some(connections) = walls.get(&current) {
                    for &next in connections {
                        if !visited.contains(&next) {
                            stack.push(next);
                        }
                    }
                }
            }
        }

        // All cells should be reachable
        assert_eq!(visited.len(), (config.width * config.height) as usize);
    }

    #[test]
    fn test_border_connections() {
        let config = MazeConfig {
            width: 5,
            height: 5,
            target_density: 1.0, // High density to force border handling
            connected: true,
            symmetry: false,
            mud_density: 0.0, // No mud for this test
            mud_range: 2,
            seed: Some(42),
        };

        let mut generator = MazeGenerator::new(config.clone());
        let (walls, _) = generator.generate();

        // Check that border cells have at least one connection
        for x in 0..config.width {
            for y in 0..config.height {
                let pos = Coordinates::new(x, y);
                if x == 0 || y == 0 || x == config.width - 1 || y == config.height - 1 {
                    assert!(walls.contains_key(&pos));
                    assert!(!walls[&pos].is_empty());
                }
            }
        }
    }

    #[test]
    fn test_basic_cheese_placement() {
        let config = CheeseConfig {
            count: 4,
            symmetry: false,
        };
        let width = 5;
        let height = 5;
        let p1 = Coordinates::new(0, 0);
        let p2 = Coordinates::new(4, 4);

        let mut generator = CheeseGenerator::new(config, width, height, Some(42));
        let cheese = generator.generate(p1, p2);

        assert_eq!(cheese.len(), 4);
        assert!(!cheese.contains(&p1));
        assert!(!cheese.contains(&p2));
    }

    #[test]
    fn test_symmetric_cheese_placement() {
        let config = CheeseConfig {
            count: 5, // Odd number
            symmetry: true,
        };
        let width = 7;
        let height = 7;
        let p1 = Coordinates::new(0, 0);
        let p2 = Coordinates::new(6, 6);

        let mut generator = CheeseGenerator::new(config, width, height, Some(42));
        let cheese = generator.generate(p1, p2);

        // Check center piece
        assert_eq!(cheese.len(), 5);
        assert!(cheese.contains(&Coordinates::new(3, 3)));

        // Verify symmetry
        for piece in &cheese {
            let symmetric = generator.get_symmetric(*piece);
            if *piece != symmetric {
                // Ignore center piece
                assert!(cheese.contains(&symmetric));
            }
        }
    }

    #[test]
    #[should_panic(expected = "Cannot place odd number of cheese")]
    fn test_invalid_symmetric_cheese() {
        let config = CheeseConfig {
            count: 5, // Odd number
            symmetry: true,
        };
        let width = 6; // Even dimensions
        let height = 6;
        let p1 = Coordinates::new(0, 0);
        let p2 = Coordinates::new(5, 5);

        let mut generator = CheeseGenerator::new(config, width, height, Some(42));
        generator.generate(p1, p2); // Should panic
    }

    #[test]
    fn test_no_cheese_on_players() {
        let config = CheeseConfig {
            count: 10,
            symmetry: false,
        };
        let width = 5;
        let height = 5;
        let p1 = Coordinates::new(0, 0);
        let p2 = Coordinates::new(4, 4);

        let mut generator = CheeseGenerator::new(config, width, height, Some(42));
        let cheese = generator.generate(p1, p2);

        assert!(
            !cheese.contains(&p1),
            "Cheese should not be placed on player 1"
        );
        assert!(
            !cheese.contains(&p2),
            "Cheese should not be placed on player 2"
        );
    }

    #[test]
    #[should_panic(expected = "Too many pieces of cheese")]
    fn test_too_many_cheese() {
        let width = 5;
        let height = 5;
        let config = CheeseConfig {
            count: 1000, // More than possible positions
            symmetry: false,
        };

        let player1_pos = Coordinates::new(0, 0);
        let player2_pos = Coordinates::new(width - 1, height - 1);

        let mut generator = CheeseGenerator::new(config, width, height, Some(42));
        generator.generate(player1_pos, player2_pos); // Should panic
    }

    #[test]
    fn test_mud_generation() {
        let config = MazeConfig {
            width: 8,
            height: 8,
            target_density: 0.7,
            connected: true,
            symmetry: false,
            mud_density: 1.0, // Always generate mud
            mud_range: 3,
            seed: Some(42),
        };

        let mut generator = MazeGenerator::new(config.clone());
        let (walls, mud) = generator.generate();

        // Every passage should have mud
        for (from, connections) in walls.iter() {
            for to in connections {
                assert!(mud.contains_key(&(*from, *to)));
                assert!(mud[&(*from, *to)] >= 2);
                assert!(mud[&(*from, *to)] <= 3);
            }
        }
    }
}
