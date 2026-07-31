#![allow(clippy::uninlined_format_args)]

use crate::{Coordinates, Direction};
use rand::prelude::IndexedRandom;
use rand::RngExt;
use std::collections::{HashMap, HashSet};

use crate::game::types::MudMap;

pub type WallMap = HashMap<Coordinates, Vec<Coordinates>>;

/// Dense passage relation used while constructing a maze.
///
/// Each cell owns one directly addressable byte whose low four bits follow
/// `Direction::{Up, Right, Down, Left}`. The unused high bits keep connection
/// updates simple; the runtime `MoveTable` remains independently packed.
struct ConnectionGrid {
    width: u8,
    height: u8,
    masks: Vec<u8>,
}

impl ConnectionGrid {
    fn new(width: u8, height: u8) -> Self {
        Self {
            width,
            height,
            masks: vec![0; usize::from(width) * usize::from(height)],
        }
    }

    #[inline]
    fn connect(&mut self, first: Coordinates, second: Coordinates) {
        debug_assert!(
            self.in_bounds(first) && self.in_bounds(second),
            "connection endpoints must be inside the grid: {first:?} -> {second:?}"
        );

        let direction = Direction::between(first, second).unwrap_or_else(|| {
            panic!("connection endpoints must be adjacent: {first:?} -> {second:?}")
        });
        let reverse = match direction {
            Direction::Up => Direction::Down,
            Direction::Right => Direction::Left,
            Direction::Down => Direction::Up,
            Direction::Left => Direction::Right,
            Direction::Stay => unreachable!("adjacent cells cannot produce Direction::Stay"),
        };

        self.masks[first.to_index(self.width)] |= 1 << direction as u8;
        self.masks[second.to_index(self.width)] |= 1 << reverse as u8;
    }

    #[inline]
    fn contains(&self, from: Coordinates, to: Coordinates) -> bool {
        if !self.in_bounds(from) || !self.in_bounds(to) {
            return false;
        }

        Direction::between(from, to).is_some_and(|direction| {
            self.masks[from.to_index(self.width)] & (1 << direction as u8) != 0
        })
    }

    #[inline]
    fn has_any(&self, pos: Coordinates) -> bool {
        self.in_bounds(pos) && self.masks[pos.to_index(self.width)] != 0
    }

    fn neighbors(&self, pos: Coordinates) -> impl Iterator<Item = Coordinates> + '_ {
        let mask = if self.in_bounds(pos) {
            self.masks[pos.to_index(self.width)]
        } else {
            0
        };

        Direction::CARDINALS
            .into_iter()
            .filter(move |&direction| mask & (1 << direction as u8) != 0)
            .map(move |direction| direction.apply_to(pos))
    }

    #[inline]
    const fn in_bounds(&self, pos: Coordinates) -> bool {
        pos.x < self.width && pos.y < self.height
    }
}

/// Configuration for maze generation
#[derive(Debug, Clone, Copy)]
pub struct MazeConfig {
    pub width: u8,
    pub height: u8,
    pub target_density: f32, // Probability of having a wall (0.0 to 1.0)
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
    connections: ConnectionGrid,
    mud: MudMap,
}

impl MazeGenerator {
    /// Creates a new maze generator with the given configuration
    #[must_use]
    pub fn new(config: MazeConfig) -> Self {
        let rng: rand::rngs::StdRng = config
            .seed
            .map_or_else(rand::make_rng, rand::SeedableRng::seed_from_u64);

        Self {
            config,
            rng,
            connections: ConnectionGrid::new(config.width, config.height),
            mud: MudMap::new(),
        }
    }

    /// Generates a complete maze with walls and mud
    pub fn generate(&mut self) -> (WallMap, MudMap) {
        let walls = self.generate_in_place();
        (walls, self.mud.clone())
    }

    /// Generates a complete maze when the caller owns this generator.
    pub(crate) fn generate_owned(mut self) -> (WallMap, MudMap) {
        let walls = self.generate_in_place();
        (walls, self.mud)
    }

    fn generate_in_place(&mut self) -> WallMap {
        self.generate_initial_layout();

        if self.config.connected {
            self.ensure_full_connectivity();
        }

        self.add_border_connections();

        // Validate before converting
        if let Err(e) = self.validate_output() {
            panic!("Maze generation failed validation: {e}");
        }

        // Convert connections to walls (blocked passages)
        self.connections_to_walls()
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
                        && self.rng.random::<f32>() >= self.config.target_density
                    {
                        let next = Coordinates::new(i + 1, j);
                        let mud_value = if self.rng.random::<f32>() < self.config.mud_density {
                            self.rng.random_range(2..=self.config.mud_range)
                        } else {
                            1 // Python uses 1 for no mud
                        };

                        // Add bidirectional connection
                        self.connections.connect(current, next);

                        if mud_value > 1 {
                            self.mud.insert(current, next, mud_value);
                        }

                        // Handle symmetry exactly as Python
                        if self.config.symmetry {
                            let sym_current = self.get_symmetric(current);
                            let sym_next = self.get_symmetric(next);

                            self.connections.connect(sym_current, sym_next);

                            if mud_value > 1 {
                                self.mud.insert(sym_current, sym_next, mud_value);
                            }
                        }
                    }

                    // Vertical connections (exactly as Python)
                    if j + 1 < self.config.height
                        && self.rng.random::<f32>() >= self.config.target_density
                    {
                        let next = Coordinates::new(i, j + 1);
                        let mud_value = if self.rng.random::<f32>() < self.config.mud_density {
                            self.rng.random_range(2..=self.config.mud_range)
                        } else {
                            1
                        };

                        self.connections.connect(current, next);

                        if mud_value > 1 {
                            self.mud.insert(current, next, mud_value);
                        }

                        if self.config.symmetry {
                            let sym_current = self.get_symmetric(current);
                            let sym_next = self.get_symmetric(next);

                            self.connections.connect(sym_current, sym_next);

                            if mud_value > 1 {
                                self.mud.insert(sym_current, sym_next, mud_value);
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

    /// Ensures the maze is fully connected by connecting all isolated regions
    fn ensure_full_connectivity(&mut self) {
        loop {
            // Find all connected components
            let mut visited = HashSet::new();
            let mut components = Vec::new();

            for x in 0..self.config.width {
                for y in 0..self.config.height {
                    let pos = Coordinates::new(x, y);
                    if !visited.contains(&pos) {
                        // Found a new component, explore it
                        let mut component = HashSet::new();
                        let mut stack = vec![pos];

                        while let Some(current) = stack.pop() {
                            if component.insert(current) {
                                visited.insert(current);

                                // Add all connected neighbors to stack
                                for next in self.connections.neighbors(current) {
                                    if !component.contains(&next) {
                                        stack.push(next);
                                    }
                                }
                            }
                        }

                        components.push(component);
                    }
                }
            }

            // If there's only one component, we're done
            if components.len() <= 1 {
                break;
            }

            // Connect the first component to another component
            let component1 = &components[0];
            let component2 = &components[1];

            // Find the closest pair of cells between the two components
            let mut best_pair = None;
            let mut min_distance = u32::MAX;

            for &pos1 in component1 {
                for &pos2 in component2 {
                    // Check if they're adjacent
                    let dx = (pos1.x as i32 - pos2.x as i32).unsigned_abs();
                    let dy = (pos1.y as i32 - pos2.y as i32).unsigned_abs();

                    if (dx == 1 && dy == 0) || (dx == 0 && dy == 1) {
                        // They're adjacent, we can connect them directly
                        best_pair = Some((pos1, pos2));
                        min_distance = 1;
                        break;
                    }

                    let distance = dx + dy;
                    if distance < min_distance {
                        min_distance = distance;
                        best_pair = Some((pos1, pos2));
                    }
                }

                if min_distance == 1 {
                    break;
                }
            }

            // Connect the two components
            if let Some((from, to)) = best_pair {
                if min_distance == 1 {
                    // They're adjacent, connect directly
                    self.add_passage(from, to);

                    if self.config.symmetry {
                        let sym_from = self.get_symmetric(from);
                        let sym_to = self.get_symmetric(to);
                        self.add_passage(sym_from, sym_to);
                    }
                } else {
                    // They're not adjacent, we need to find a path
                    // For simplicity, just ensure the old algorithm runs
                    self.ensure_connectivity();
                }
            }
        }
    }

    /// Ensures the maze is fully connected using a modified DFS algorithm
    fn ensure_connectivity(&mut self) {
        let mut connected =
            vec![vec![false; self.config.height as usize]; self.config.width as usize];
        let mut possible_border = Vec::new();

        // Start from top-left corner (0,0)
        let start = Coordinates::new(0, 0);
        connected[0][0] = true;
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
            let idx = self.rng.random_range(0..border.len());
            let (from, to) = border[idx];

            // Generate mud exactly as Python
            let mud_value = if self.rng.random::<f32>() < self.config.mud_density {
                self.rng.random_range(2..=self.config.mud_range)
            } else {
                1
            };

            // Add connections
            self.connections.connect(from, to);

            if mud_value > 1 {
                self.mud.insert(from, to, mud_value);
            }

            // Handle symmetry exactly as Python
            if self.config.symmetry {
                let sym_from = self.get_symmetric(from);
                let sym_to = self.get_symmetric(to);

                self.connections.connect(sym_from, sym_to);

                if mud_value > 1 {
                    self.mud.insert(sym_from, sym_to, mud_value);
                }
            }

            connected[to.x as usize][to.y as usize] = true;
            possible_border.push(to);
            *possible_border = new_possible_border;
        }
    }
    #[inline]
    fn has_connection(&self, from: Coordinates, to: Coordinates) -> bool {
        self.connections.contains(from, to)
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
        self.connections.connect(from, to);

        // Then handle mud if needed
        if self.rng.random::<f32>() < self.config.mud_density {
            let mud_value = self.rng.random_range(2..=self.config.mud_range);

            // MudMap stores both orientations for the passage.
            self.mud.insert(from, to, mud_value);

            // If symmetric, add mud for the symmetric passage
            if self.config.symmetry {
                let sym_from = self.get_symmetric(from);
                let sym_to = self.get_symmetric(to);
                self.mud.insert(sym_from, sym_to, mud_value);
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
        self.connections.has_any(pos)
    }

    /// Converts internal connections representation to walls (blocked passages)
    /// The internal representation uses connections (where you CAN move)
    /// But the game expects walls (where you CANNOT move)
    fn connections_to_walls(&self) -> WallMap {
        let mut walls = HashMap::new();

        // For each position, check all four directions
        for x in 0..self.config.width {
            for y in 0..self.config.height {
                let current = Coordinates::new(x, y);

                // Check all four adjacent cells
                let adjacent = [
                    (x.saturating_sub(1), y, x > 0),                      // Left
                    (x.saturating_add(1), y, x + 1 < self.config.width),  // Right
                    (x, y.saturating_sub(1), y > 0),                      // Down
                    (x, y.saturating_add(1), y + 1 < self.config.height), // Up
                ];

                for (adj_x, adj_y, in_bounds) in adjacent {
                    if in_bounds {
                        let adjacent = Coordinates::new(adj_x, adj_y);

                        // If there's no connection, there's a wall
                        if !self.has_connection(current, adjacent) {
                            walls.entry(current).or_insert_with(Vec::new).push(adjacent);
                        }
                    }
                }
            }
        }

        walls
    }

    /// Validates the generated maze output
    fn validate_output(&self) -> Result<(), String> {
        // Check 1: Every mud entry must correspond to a connection (passage)
        for ((from, to), mud_value) in self.mud.iter() {
            if !self.has_connection(from, to) {
                return Err(format!(
                    "Mud exists between {:?} and {:?} with value {}, but there's no connection",
                    from, to, mud_value
                ));
            }
        }

        // `ConnectionGrid::connect` establishes these structural invariants.
        // Recheck them in debug/test builds without charging release generation.
        #[cfg(debug_assertions)]
        {
            for x in 0..self.config.width {
                for y in 0..self.config.height {
                    let from = Coordinates::new(x, y);
                    for to in self.connections.neighbors(from) {
                        if !self.connections.in_bounds(to) {
                            return Err(format!("Connection to out-of-bounds position {:?}", to));
                        }
                        if !self.has_connection(to, from) {
                            return Err(format!(
                                "Connection from {:?} to {:?} is not bidirectional",
                                from, to
                            ));
                        }
                    }
                }
            }
        }

        // Check 3: If connected mode, verify full connectivity
        if self.config.connected {
            let mut visited = HashSet::new();
            let mut stack = vec![Coordinates::new(0, 0)];

            while let Some(current) = stack.pop() {
                if visited.insert(current) {
                    for next in self.connections.neighbors(current) {
                        if !visited.contains(&next) {
                            stack.push(next);
                        }
                    }
                }
            }

            let total_cells = usize::from(self.config.width) * usize::from(self.config.height);
            if visited.len() != total_cells {
                return Err(format!(
                    "Maze is not fully connected. Visited {} cells out of {}",
                    visited.len(),
                    total_cells
                ));
            }
        }

        Ok(())
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
        let rng: rand::rngs::StdRng =
            seed.map_or_else(rand::make_rng, rand::SeedableRng::seed_from_u64);

        Self {
            config,
            rng,
            width,
            height,
        }
    }

    /// Generate cheese placements.
    ///
    /// # Errors
    /// - When attempting to place odd number of cheese in symmetric maze with even dimensions
    /// - When requesting more cheese pieces than available positions in the maze
    pub fn generate(
        &mut self,
        player1_pos: Coordinates,
        player2_pos: Coordinates,
    ) -> Result<Vec<Coordinates>, String> {
        let mut pieces = Vec::new();
        let mut remaining = self.config.count;

        // Handle center piece for odd counts in symmetric mazes
        if self.config.symmetry && remaining % 2 == 1 {
            if self.width.is_multiple_of(2) || self.height.is_multiple_of(2) {
                return Err(
                    "Cannot place odd number of cheese in symmetric maze with even dimensions"
                        .to_string(),
                );
            }
            let center = Coordinates::new(self.width / 2, self.height / 2);
            if center != player1_pos && center != player2_pos {
                pieces.push(center);
                remaining -= 1;
            }
        }

        // Generate candidate positions
        let mut candidates = Vec::new();

        for x in 0..self.width {
            for y in 0..self.height {
                let pos = Coordinates::new(x, y);
                let symmetric = self.get_symmetric(pos);
                let precedes_symmetric =
                    pos.x < symmetric.x || (pos.x == symmetric.x && pos.y < symmetric.y);
                let represents_pair = !self.config.symmetry
                    || precedes_symmetric
                    // Preserve which half represents the pair when its earlier
                    // coordinate is occupied by a player.
                    || symmetric == player1_pos
                    || symmetric == player2_pos;

                if represents_pair && pos != player1_pos && pos != player2_pos && pos != symmetric {
                    candidates.push(pos);
                }
            }
        }

        // Place remaining pieces
        while remaining > 0 && !candidates.is_empty() {
            let idx = self.rng.random_range(0..candidates.len());
            let chosen = candidates.swap_remove(idx);
            pieces.push(chosen);

            if self.config.symmetry {
                let symmetric = self.get_symmetric(chosen);
                pieces.push(symmetric);
                remaining -= 2;
            } else {
                remaining -= 1;
            }
        }

        if remaining != 0 {
            return Err("Too many pieces of cheese for maze dimensions".to_string());
        }

        Ok(pieces)
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

    fn legacy_generate_cheese(
        config: &CheeseConfig,
        width: u8,
        height: u8,
        players: (Coordinates, Coordinates),
        rng: &mut rand::rngs::StdRng,
    ) -> Result<Vec<Coordinates>, String> {
        let symmetric = |pos: Coordinates| Coordinates::new(width - 1 - pos.x, height - 1 - pos.y);
        let mut pieces = Vec::new();
        let mut remaining = config.count;

        if config.symmetry && remaining % 2 == 1 {
            if width.is_multiple_of(2) || height.is_multiple_of(2) {
                return Err(
                    "Cannot place odd number of cheese in symmetric maze with even dimensions"
                        .to_string(),
                );
            }
            let center = Coordinates::new(width / 2, height / 2);
            if center != players.0 && center != players.1 {
                pieces.push(center);
                remaining -= 1;
            }
        }

        let mut candidates = Vec::new();
        let mut considered = HashSet::new();
        for x in 0..width {
            for y in 0..height {
                let pos = Coordinates::new(x, y);
                if (!config.symmetry || !considered.contains(&pos))
                    && pos != players.0
                    && pos != players.1
                    && pos != symmetric(pos)
                {
                    candidates.push(pos);
                    if config.symmetry {
                        considered.insert(pos);
                        considered.insert(symmetric(pos));
                    }
                }
            }
        }

        while remaining > 0 && !candidates.is_empty() {
            let idx = rng.random_range(0..candidates.len());
            let chosen = candidates.swap_remove(idx);
            pieces.push(chosen);

            if config.symmetry {
                let symmetric = symmetric(chosen);
                pieces.push(symmetric);
                candidates.retain(|&pos| pos != symmetric);
                remaining -= 2;
            } else {
                remaining -= 1;
            }
        }

        if remaining != 0 {
            return Err("Too many pieces of cheese for maze dimensions".to_string());
        }

        Ok(pieces)
    }

    /// Test-only copy of the removed hash-backed path for the deterministic
    /// `connected = false` compatibility boundary.
    struct LegacyDisconnectedMazeGenerator {
        config: MazeConfig,
        rng: rand::rngs::StdRng,
        connections: HashMap<Coordinates, Vec<Coordinates>>,
        mud: MudMap,
    }

    impl LegacyDisconnectedMazeGenerator {
        fn new(config: MazeConfig) -> Self {
            assert!(!config.connected);
            let rng = config
                .seed
                .map_or_else(rand::make_rng, rand::SeedableRng::seed_from_u64);
            Self {
                config,
                rng,
                connections: HashMap::new(),
                mud: MudMap::new(),
            }
        }

        fn generate(&mut self) -> (WallMap, MudMap) {
            self.generate_initial_layout();
            self.add_border_connections();
            (self.connections_to_walls(), self.mud.clone())
        }

        fn generate_initial_layout(&mut self) {
            let mut not_considered = HashSet::new();
            for x in 0..self.config.width {
                for y in 0..self.config.height {
                    not_considered.insert(Coordinates::new(x, y));
                }
            }

            for x in 0..self.config.width {
                for y in 0..self.config.height {
                    let current = Coordinates::new(x, y);
                    if self.config.symmetry && !not_considered.contains(&current) {
                        continue;
                    }

                    if x + 1 < self.config.width
                        && self.rng.random::<f32>() >= self.config.target_density
                    {
                        let next = Coordinates::new(x + 1, y);
                        let mud_value = self.random_initial_mud();
                        self.record_passage(current, next, mud_value);
                        if self.config.symmetry {
                            self.record_passage(
                                self.get_symmetric(current),
                                self.get_symmetric(next),
                                mud_value,
                            );
                        }
                    }

                    if y + 1 < self.config.height
                        && self.rng.random::<f32>() >= self.config.target_density
                    {
                        let next = Coordinates::new(x, y + 1);
                        let mud_value = self.random_initial_mud();
                        self.record_passage(current, next, mud_value);
                        if self.config.symmetry {
                            self.record_passage(
                                self.get_symmetric(current),
                                self.get_symmetric(next),
                                mud_value,
                            );
                        }
                    }

                    if self.config.symmetry {
                        not_considered.remove(&current);
                        not_considered.remove(&self.get_symmetric(current));
                    }
                }
            }
        }

        fn random_initial_mud(&mut self) -> u8 {
            if self.rng.random::<f32>() < self.config.mud_density {
                self.rng.random_range(2..=self.config.mud_range)
            } else {
                1
            }
        }

        fn record_passage(&mut self, from: Coordinates, to: Coordinates, mud_value: u8) {
            self.connect(from, to);
            if mud_value > 1 {
                self.mud.insert(from, to, mud_value);
            }
        }

        fn connect(&mut self, from: Coordinates, to: Coordinates) {
            self.connections.entry(from).or_default().push(to);
            self.connections.entry(to).or_default().push(from);
        }

        fn add_border_connections(&mut self) {
            for x in 0..self.config.width {
                for y in 0..self.config.height {
                    let current = Coordinates::new(x, y);
                    if self.is_border_cell(current) && !self.has_any_connection(current) {
                        let neighbors = self.get_valid_neighbors(current);
                        if let Some(&neighbor) = neighbors.choose(&mut self.rng) {
                            self.add_passage(current, neighbor);
                            if self.config.symmetry {
                                self.add_passage(
                                    self.get_symmetric(current),
                                    self.get_symmetric(neighbor),
                                );
                            }
                        }
                    }
                }
            }
        }

        fn add_passage(&mut self, from: Coordinates, to: Coordinates) {
            self.connect(from, to);
            if self.rng.random::<f32>() < self.config.mud_density {
                let mud_value = self.rng.random_range(2..=self.config.mud_range);
                self.mud.insert(from, to, mud_value);
                if self.config.symmetry {
                    self.mud
                        .insert(self.get_symmetric(from), self.get_symmetric(to), mud_value);
                }
            }
        }

        fn get_valid_neighbors(&self, pos: Coordinates) -> Vec<Coordinates> {
            let mut neighbors = Vec::new();
            let directions = [(0, 1), (1, 0), (0, -1), (-1, 0)];

            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            for (dx, dy) in directions {
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

        fn connections_to_walls(&self) -> WallMap {
            let mut walls = HashMap::new();
            for x in 0..self.config.width {
                for y in 0..self.config.height {
                    let current = Coordinates::new(x, y);
                    let adjacent = [
                        (x.saturating_sub(1), y, x > 0),
                        (x.saturating_add(1), y, x + 1 < self.config.width),
                        (x, y.saturating_sub(1), y > 0),
                        (x, y.saturating_add(1), y + 1 < self.config.height),
                    ];

                    for (adjacent_x, adjacent_y, in_bounds) in adjacent {
                        if in_bounds {
                            let adjacent = Coordinates::new(adjacent_x, adjacent_y);
                            if !self
                                .connections
                                .get(&current)
                                .is_some_and(|neighbors| neighbors.contains(&adjacent))
                            {
                                walls.entry(current).or_insert_with(Vec::new).push(adjacent);
                            }
                        }
                    }
                }
            }
            walls
        }

        fn get_symmetric(&self, pos: Coordinates) -> Coordinates {
            Coordinates::new(
                self.config.width - 1 - pos.x,
                self.config.height - 1 - pos.y,
            )
        }

        fn is_border_cell(&self, pos: Coordinates) -> bool {
            pos.x == 0
                || pos.y == 0
                || pos.x == self.config.width - 1
                || pos.y == self.config.height - 1
        }

        fn has_any_connection(&self, pos: Coordinates) -> bool {
            self.connections
                .get(&pos)
                .is_some_and(|neighbors| !neighbors.is_empty())
        }
    }

    fn sorted_mud_entries(mud: &MudMap) -> Vec<((Coordinates, Coordinates), u8)> {
        let mut entries: Vec<_> = mud.iter().collect();
        entries.sort_unstable();
        entries
    }

    #[test]
    fn connection_grid_stores_bidirectional_stable_neighbors() {
        let mut grid = ConnectionGrid::new(3, 3);
        let center = Coordinates::new(1, 1);
        let up = Coordinates::new(1, 2);
        let right = Coordinates::new(2, 1);
        let down = Coordinates::new(1, 0);
        let left = Coordinates::new(0, 1);

        for neighbor in [left, down, right, up, right] {
            grid.connect(center, neighbor);
        }

        assert!(grid.has_any(center));
        assert_eq!(
            grid.neighbors(center).collect::<Vec<_>>(),
            vec![up, right, down, left]
        );
        for neighbor in [up, right, down, left] {
            assert!(grid.contains(center, neighbor));
            assert!(grid.contains(neighbor, center));
        }

        let corner = Coordinates::new(0, 0);
        grid.connect(corner, Coordinates::new(0, 1));
        grid.connect(corner, Coordinates::new(1, 0));
        assert_eq!(
            grid.neighbors(corner).collect::<Vec<_>>(),
            vec![Coordinates::new(0, 1), Coordinates::new(1, 0)]
        );
        assert!(!grid.contains(corner, Coordinates::new(1, 1)));
        assert!(!grid.contains(corner, Coordinates::new(3, 0)));
        assert!(!grid.has_any(Coordinates::new(3, 0)));
    }

    #[test]
    fn disconnected_generation_matches_legacy_hash_backed_path() {
        let cases = [
            (1, 1, 1.0, false, 0.0, 2),
            (2, 1, 1.0, true, 1.0, 4),
            (4, 3, 0.35, false, 0.65, 4),
            (5, 4, 0.55, true, 0.35, 5),
            (4, 5, 0.0, true, 0.0, 2),
            (6, 4, 1.0, true, 0.5, 3),
        ];

        for (width, height, target_density, symmetry, mud_density, mud_range) in cases {
            for seed in [0, 1, 42, u64::MAX] {
                let config = MazeConfig {
                    width,
                    height,
                    target_density,
                    connected: false,
                    symmetry,
                    mud_density,
                    mud_range,
                    seed: Some(seed),
                };
                let mut dense = MazeGenerator::new(config);
                let mut legacy = LegacyDisconnectedMazeGenerator::new(config);

                for generation in 1..=2 {
                    let (dense_walls, dense_mud) = dense.generate();
                    let (legacy_walls, legacy_mud) = legacy.generate();
                    assert_eq!(
                        dense_walls, legacy_walls,
                        "wall mismatch for {width}x{height}, symmetry={symmetry}, seed={seed}, generation={generation}"
                    );
                    assert_eq!(
                        sorted_mud_entries(&dense_mud),
                        sorted_mud_entries(&legacy_mud),
                        "mud mismatch for {width}x{height}, symmetry={symmetry}, seed={seed}, generation={generation}"
                    );
                }
            }
        }
    }

    fn wall_between(walls: &WallMap, from: Coordinates, to: Coordinates) -> bool {
        walls
            .get(&from)
            .is_some_and(|neighbors| neighbors.contains(&to))
    }

    fn in_bounds(config: MazeConfig, pos: Coordinates) -> bool {
        pos.x < config.width && pos.y < config.height
    }

    fn assert_connected_generation_invariants(config: MazeConfig, walls: &WallMap, mud: &MudMap) {
        for (&from, blocked) in walls {
            assert!(in_bounds(config, from));
            for &to in blocked {
                assert!(in_bounds(config, to));
                assert!(Direction::between(from, to).is_some());
                assert!(wall_between(walls, to, from));
            }
        }

        for x in 0..config.width {
            for y in 0..config.height {
                let from = Coordinates::new(x, y);
                for direction in Direction::CARDINALS {
                    let to = direction.apply_to(from);
                    if !in_bounds(config, to) || to == from {
                        continue;
                    }

                    assert_eq!(
                        wall_between(walls, from, to),
                        wall_between(walls, to, from),
                        "wall direction mismatch at {from:?} -> {to:?}"
                    );
                    if config.symmetry {
                        let symmetric_from =
                            Coordinates::new(config.width - 1 - from.x, config.height - 1 - from.y);
                        let symmetric_to =
                            Coordinates::new(config.width - 1 - to.x, config.height - 1 - to.y);
                        assert_eq!(
                            wall_between(walls, from, to),
                            wall_between(walls, symmetric_from, symmetric_to),
                            "wall symmetry mismatch at {from:?} -> {to:?}"
                        );
                    }
                }
            }
        }

        let mut visited = HashSet::new();
        let mut stack = vec![Coordinates::new(0, 0)];
        while let Some(from) = stack.pop() {
            if !visited.insert(from) {
                continue;
            }
            for direction in Direction::CARDINALS {
                let to = direction.apply_to(from);
                if in_bounds(config, to)
                    && to != from
                    && !wall_between(walls, from, to)
                    && !visited.contains(&to)
                {
                    stack.push(to);
                }
            }
        }
        assert_eq!(
            visited.len(),
            usize::from(config.width) * usize::from(config.height)
        );

        for ((from, to), cost) in mud.iter() {
            assert!(in_bounds(config, from));
            assert!(in_bounds(config, to));
            assert!(Direction::between(from, to).is_some());
            assert!(!wall_between(walls, from, to));
            assert_eq!(mud.get(to, from), Some(cost));
            assert!((2..=config.mud_range).contains(&cost));

            if config.symmetry {
                let symmetric_from =
                    Coordinates::new(config.width - 1 - from.x, config.height - 1 - from.y);
                let symmetric_to =
                    Coordinates::new(config.width - 1 - to.x, config.height - 1 - to.y);
                assert_eq!(mud.get(symmetric_from, symmetric_to), Some(cost));
            }
        }
    }

    #[test]
    fn connected_generation_preserves_grid_and_mud_invariants() {
        for (width, height, symmetry) in [(3, 3, false), (4, 3, false), (5, 4, true), (4, 5, true)]
        {
            for target_density in [0.0, 0.7, 1.0] {
                for seed in 0..4 {
                    let config = MazeConfig {
                        width,
                        height,
                        target_density,
                        connected: true,
                        symmetry,
                        mud_density: 0.55,
                        mud_range: 4,
                        seed: Some(seed),
                    };
                    let (walls, mud) = MazeGenerator::new(config).generate_owned();
                    assert_connected_generation_invariants(config, &walls, &mud);
                }
            }
        }
    }

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
    fn owned_generation_matches_borrowed_generation() {
        let config = MazeConfig {
            width: 5,
            height: 4,
            target_density: 0.55,
            connected: false,
            symmetry: true,
            mud_density: 0.65,
            mud_range: 4,
            seed: Some(0xA11C_E5E5),
        };

        let mut borrowed_generator = MazeGenerator::new(config);
        let (borrowed_walls, borrowed_mud) = borrowed_generator.generate();
        let (owned_walls, owned_mud) = MazeGenerator::new(config).generate_owned();
        let mut borrowed_mud: Vec<_> = borrowed_mud.iter().collect();
        let mut owned_mud: Vec<_> = owned_mud.iter().collect();
        borrowed_mud.sort_unstable();
        owned_mud.sort_unstable();

        assert_eq!(owned_walls, borrowed_walls);
        assert_eq!(owned_mud, borrowed_mud);
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

        let mut generator = MazeGenerator::new(config);
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
            assert_eq!(mud.get(sym_from, sym_to), Some(value));
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

        let mut generator = MazeGenerator::new(config);
        let (walls, _) = generator.generate();

        // Check if all cells are reachable from starting position
        // We need to check connections, not walls, so we need to reconstruct the connections
        let mut connections = HashMap::new();

        // Build connections from walls (walls block movement, so where there's no wall, there's a connection)
        for x in 0..config.width {
            for y in 0..config.height {
                let current = Coordinates::new(x, y);
                let mut current_connections = Vec::new();

                // Check all four directions
                let adjacent = [
                    (x.saturating_sub(1), y, x > 0),                 // Left
                    (x.saturating_add(1), y, x + 1 < config.width),  // Right
                    (x, y.saturating_sub(1), y > 0),                 // Down
                    (x, y.saturating_add(1), y + 1 < config.height), // Up
                ];

                for (adj_x, adj_y, in_bounds) in adjacent {
                    if in_bounds {
                        let adjacent = Coordinates::new(adj_x, adj_y);
                        // If there's no wall blocking this direction, there's a connection
                        if !walls
                            .get(&current)
                            .is_some_and(|blocked| blocked.contains(&adjacent))
                        {
                            current_connections.push(adjacent);
                        }
                    }
                }

                if !current_connections.is_empty() {
                    connections.insert(current, current_connections);
                }
            }
        }

        let mut visited = HashSet::new();
        let mut stack = vec![Coordinates::new(0, 0)];

        while let Some(current) = stack.pop() {
            if visited.insert(current) {
                if let Some(conns) = connections.get(&current) {
                    for &next in conns {
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
            target_density: 1.0, // Maximum wall density - should still be connected
            connected: true,
            symmetry: false,
            mud_density: 0.0, // No mud for this test
            mud_range: 2,
            seed: Some(42),
        };

        let mut generator = MazeGenerator::new(config);
        let (walls, _) = generator.generate();

        // With high wall density (1.0), we should have many walls
        // The exact count depends on connectivity requirements
        // But we should have at least some walls in the maze
        assert!(
            !walls.is_empty(),
            "High density maze should have some walls"
        );

        // Check that the maze is still connected despite high wall density
        // This ensures the connectivity algorithm is working properly
        let mut visited = HashSet::new();
        let mut stack = vec![Coordinates::new(0, 0)];

        // Build connections map from walls (inverse of walls)
        let mut connections = HashMap::new();
        for x in 0..config.width {
            for y in 0..config.height {
                let current = Coordinates::new(x, y);
                let mut current_connections = Vec::new();

                let adjacent = [
                    (x.saturating_sub(1), y, x > 0),
                    (x.saturating_add(1), y, x + 1 < config.width),
                    (x, y.saturating_sub(1), y > 0),
                    (x, y.saturating_add(1), y + 1 < config.height),
                ];

                for (adj_x, adj_y, in_bounds) in adjacent {
                    if in_bounds {
                        let adjacent = Coordinates::new(adj_x, adj_y);
                        if !walls
                            .get(&current)
                            .is_some_and(|blocked| blocked.contains(&adjacent))
                        {
                            current_connections.push(adjacent);
                        }
                    }
                }

                if !current_connections.is_empty() {
                    connections.insert(current, current_connections);
                }
            }
        }

        // Traverse connections
        while let Some(current) = stack.pop() {
            if visited.insert(current) {
                if let Some(conns) = connections.get(&current) {
                    for &next in conns {
                        if !visited.contains(&next) {
                            stack.push(next);
                        }
                    }
                }
            }
        }

        // Despite high wall density, all cells should still be reachable
        assert_eq!(visited.len(), (config.width * config.height) as usize);
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
        let cheese = generator.generate(p1, p2).unwrap();

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
        let cheese = generator.generate(p1, p2).unwrap();

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
    fn test_seeded_symmetric_cheese_sequence_is_stable() {
        let config = CheeseConfig {
            count: 9,
            symmetry: true,
        };
        let mut generator = CheeseGenerator::new(config, 7, 5, Some(42));
        let player1 = Coordinates::new(0, 0);
        let player2 = Coordinates::new(6, 4);

        let first = generator.generate(player1, player2).unwrap();
        let second = generator.generate(player1, player2).unwrap();

        assert_eq!(
            first,
            vec![
                Coordinates::new(3, 2),
                Coordinates::new(0, 3),
                Coordinates::new(6, 1),
                Coordinates::new(1, 3),
                Coordinates::new(5, 1),
                Coordinates::new(0, 4),
                Coordinates::new(6, 0),
                Coordinates::new(3, 0),
                Coordinates::new(3, 4),
            ]
        );
        assert_eq!(
            second,
            vec![
                Coordinates::new(3, 2),
                Coordinates::new(2, 4),
                Coordinates::new(4, 0),
                Coordinates::new(2, 0),
                Coordinates::new(4, 4),
                Coordinates::new(3, 1),
                Coordinates::new(3, 3),
                Coordinates::new(1, 1),
                Coordinates::new(5, 3),
            ]
        );
    }

    #[test]
    fn test_cheese_generation_matches_legacy_bookkeeping() {
        let cases = [
            (7, false, 5, 4, (0, 0), (4, 3)),
            (9, true, 7, 5, (0, 0), (6, 4)),
            (8, true, 6, 4, (0, 0), (5, 3)),
            (6, true, 5, 5, (0, 1), (3, 4)),
            (100, true, 5, 5, (0, 0), (4, 4)),
        ];

        for (count, symmetry, width, height, player1, player2) in cases {
            let config = CheeseConfig { count, symmetry };
            let players = (
                Coordinates::new(player1.0, player1.1),
                Coordinates::new(player2.0, player2.1),
            );
            for seed in [0, 1, 42, u64::MAX] {
                let mut generator = CheeseGenerator::new(config.clone(), width, height, Some(seed));
                let mut legacy_rng = rand::SeedableRng::seed_from_u64(seed);

                for generation in 0..2 {
                    assert_eq!(
                        generator.generate(players.0, players.1),
                        legacy_generate_cheese(&config, width, height, players, &mut legacy_rng,),
                        "width={width}, height={height}, seed={seed}, generation={generation}",
                    );
                }
            }
        }
    }

    #[test]
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
        let result = generator.generate(p1, p2);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Cannot place odd number of cheese"));
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
        let cheese = generator.generate(p1, p2).unwrap();

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
        let result = generator.generate(player1_pos, player2_pos);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Too many pieces of cheese"));
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

        let mut generator = MazeGenerator::new(config);
        let (walls, mud) = generator.generate();

        // Mud should only exist on passages (where there are no walls)
        // First, verify no mud exists on walls
        for ((from, to), _) in mud.iter() {
            // Check that there's no wall between these positions
            let has_wall = walls
                .get(&from)
                .is_some_and(|blocked| blocked.contains(&to))
                || walls
                    .get(&to)
                    .is_some_and(|blocked| blocked.contains(&from));
            assert!(
                !has_wall,
                "Mud exists on a wall between {:?} and {:?}",
                from, to
            );
        }

        // With mud_density = 1.0, we should have mud on many passages
        // But we can't check exact count since connectivity affects passage count
        assert!(
            !mud.is_empty(),
            "Should have at least some mud with density 1.0"
        );

        // Check mud values are in correct range
        for (_, value) in mud.iter() {
            assert!(value >= 2);
            assert!(value <= 3);
        }
    }
}
