#[cfg(feature = "python")]
use pyo3::exceptions::PyValueError;
#[cfg(feature = "python")]
use pyo3::prelude::*;
#[cfg(feature = "python")]
use pyo3::types::PyTuple;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[cfg_attr(
    feature = "python",
    pyclass(module = "pyrat_engine._core.types", frozen, get_all)
)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Deserialize, Serialize)]
pub struct Coordinates {
    pub x: u8,
    pub y: u8,
}

impl Coordinates {
    #[must_use]
    #[inline(always)]
    pub const fn new(x: u8, y: u8) -> Self {
        Self { x, y }
    }

    #[must_use]
    #[inline(always)]
    pub const fn to_index(&self, width: u8) -> usize {
        (self.y as usize) * (width as usize) + (self.x as usize)
    }
}

// Python-specific methods
#[cfg(feature = "python")]
#[pymethods]
impl Coordinates {
    #[new]
    fn py_new(x: i32, y: i32) -> PyResult<Self> {
        if x < 0 || y < 0 {
            return Err(PyValueError::new_err(format!(
                "Coordinates({x}, {y}) - coordinates cannot be negative"
            )));
        }
        if x > 255 || y > 255 {
            return Err(PyValueError::new_err(format!(
                "Coordinates({x}, {y}) - coordinates cannot exceed 255"
            )));
        }
        Ok(Self::new(x as u8, y as u8))
    }

    fn get_neighbor(&self, direction: u8) -> PyResult<Self> {
        let dir = Direction::try_from(direction)
            .map_err(|_| PyValueError::new_err("Invalid direction value"))?;
        Ok(dir.apply_to(*self))
    }

    fn is_adjacent_to(&self, other: &Self) -> bool {
        let dx = self.x.abs_diff(other.x);
        let dy = self.y.abs_diff(other.y);
        (dx == 1 && dy == 0) || (dx == 0 && dy == 1)
    }

    fn manhattan_distance(&self, other: &Self) -> u16 {
        self.x.abs_diff(other.x) as u16 + self.y.abs_diff(other.y) as u16
    }

    pub fn __repr__(&self) -> String {
        format!("Coordinates({}, {})", self.x, self.y)
    }

    fn __str__(&self) -> String {
        format!("({}, {})", self.x, self.y)
    }

    fn __getitem__(&self, index: isize) -> PyResult<u8> {
        let normalized_index = if index < 0 {
            // Handle negative indices
            if index >= -2 {
                (2 + index) as usize
            } else {
                return Err(pyo3::exceptions::PyIndexError::new_err(
                    "Index out of range",
                ));
            }
        } else {
            index as usize
        };

        match normalized_index {
            0 => Ok(self.x),
            1 => Ok(self.y),
            _ => Err(pyo3::exceptions::PyIndexError::new_err(
                "Index out of range",
            )),
        }
    }

    fn __len__(&self) -> usize {
        2
    }

    fn __hash__(&self) -> u64 {
        // Simple hash combining x and y
        ((self.x as u64) << 8) | (self.y as u64)
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.x == other.x && self.y == other.y
    }

    fn __ne__(&self, other: &Self) -> bool {
        !self.__eq__(other)
    }

    fn __add__(&self, other: CoordinatesAddInput) -> PyResult<Self> {
        match other {
            CoordinatesAddInput::Tuple(dx, dy) => {
                // Apply delta with saturation
                let new_x = if dx >= 0 {
                    self.x.saturating_add(dx as u8)
                } else {
                    self.x.saturating_sub((-dx) as u8)
                };
                let new_y = if dy >= 0 {
                    self.y.saturating_add(dy as u8)
                } else {
                    self.y.saturating_sub((-dy) as u8)
                };
                Ok(Self::new(new_x, new_y))
            },
            CoordinatesAddInput::Direction(dir) => {
                let direction = Direction::try_from(dir).map_err(|_| {
                    PyValueError::new_err(format!(
                        "Invalid direction value: {dir}. Expected 0-4 (UP, RIGHT, DOWN, LEFT, STAY)."
                    ))
                })?;
                Ok(direction.apply_to(*self))
            },
        }
    }

    fn __sub__(&self, other: CoordinatesInput) -> PyResult<(i32, i32)> {
        let other_coord: Coordinates = PyResult::<Coordinates>::from(other)?;
        Ok((
            self.x as i32 - other_coord.x as i32,
            self.y as i32 - other_coord.y as i32,
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum Direction {
    Up = 0,
    Right = 1,
    Down = 2,
    Left = 3,
    Stay = 4, // Special case, not stored in move table
}

impl Direction {
    /// The four cardinal directions (excludes Stay).
    pub const CARDINALS: [Direction; 4] = [
        Direction::Up,
        Direction::Right,
        Direction::Down,
        Direction::Left,
    ];

    /// Derive the direction needed to move from `a` to `b`.
    /// Returns `None` if the cells aren't orthogonally adjacent.
    pub fn between(from: Coordinates, to: Coordinates) -> Option<Direction> {
        let dx = to.x as i16 - from.x as i16;
        let dy = to.y as i16 - from.y as i16;
        match (dx, dy) {
            (0, 1) => Some(Direction::Up),
            (0, -1) => Some(Direction::Down),
            (1, 0) => Some(Direction::Right),
            (-1, 0) => Some(Direction::Left),
            _ => None,
        }
    }

    /// Apply move in the mathematical coordinate system where:
    /// - x increases to the right
    /// - y increases going up
    /// - (0,0) is at the bottom-left corner
    #[inline(always)]
    pub const fn apply_to(&self, pos: Coordinates) -> Coordinates {
        match self {
            Self::Up => Coordinates {
                x: pos.x,
                y: pos.y.saturating_add(1), // Up means increasing y
            },
            Self::Down => Coordinates {
                x: pos.x,
                y: pos.y.saturating_sub(1), // Down means decreasing y
            },
            Self::Left => Coordinates {
                x: pos.x.saturating_sub(1),
                y: pos.y,
            },
            Self::Right => Coordinates {
                x: pos.x.saturating_add(1),
                y: pos.y,
            },
            Self::Stay => pos,
        }
    }
}

impl TryFrom<u8> for Direction {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Up),
            1 => Ok(Self::Right),
            2 => Ok(Self::Down),
            3 => Ok(Self::Left),
            4 => Ok(Self::Stay),
            _ => Err("Invalid direction value"),
        }
    }
}

type MudKey = (Coordinates, Coordinates);
type MudEntry = (MudKey, u8);

/// A wrapper around bidirectional mud lookups.
///
/// Maps are mutable while they are being built. Once attached to a maze they
/// become a compact, shared lookup table; mutating a shared map transparently
/// thaws only that clone back into an owned `HashMap`.
#[derive(Clone)]
pub struct MudMap {
    storage: MudStorage,
}

#[derive(Clone)]
enum MudStorage {
    Mutable(HashMap<MudKey, u8>),
    Frozen(Arc<FrozenMudMap>),
}

struct FrozenMudMap {
    width: u8,
    height: u8,
    costs: Box<[u8]>,
    present: Box<[u64]>,
    entries: Box<[MudEntry]>,
    overflow: HashMap<MudKey, u8>,
}

enum MudMapIterInner<'a> {
    Mutable(std::collections::hash_map::Iter<'a, MudKey, u8>),
    Frozen(std::slice::Iter<'a, MudEntry>),
}

struct MudMapIter<'a> {
    inner: MudMapIterInner<'a>,
}

impl Iterator for MudMapIter<'_> {
    type Item = MudEntry;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            MudMapIterInner::Mutable(entries) => loop {
                let (&(pos1, pos2), &value) = entries.next()?;
                if pos1 < pos2 {
                    return Some(((pos1, pos2), value));
                }
            },
            MudMapIterInner::Frozen(entries) => entries.next().copied(),
        }
    }
}

impl FrozenMudMap {
    fn from_mutable(width: u8, height: u8, mutable: HashMap<MudKey, u8>) -> Self {
        let mut entries: Vec<_> = mutable
            .iter()
            .filter(|((pos1, pos2), _)| pos1 < pos2)
            .map(|(&(pos1, pos2), &value)| ((pos1, pos2), value))
            .collect();
        entries.sort_unstable();

        let has_dense_entries = mutable
            .keys()
            .any(|&(pos1, pos2)| mud_slot(width, height, pos1, pos2).is_some());
        let slot_count = usize::from(width) * usize::from(height) * 4;
        let mut costs = if has_dense_entries {
            vec![0; slot_count]
        } else {
            Vec::new()
        };
        let mut present = if has_dense_entries {
            vec![0; slot_count.div_ceil(64)]
        } else {
            Vec::new()
        };
        let mut overflow = HashMap::new();

        for (&(pos1, pos2), &value) in &mutable {
            if let Some(slot) = mud_slot(width, height, pos1, pos2) {
                costs[slot] = value;
                present[slot / 64] |= 1 << (slot % 64);
            } else {
                overflow.insert((pos1, pos2), value);
            }
        }

        Self {
            width,
            height,
            costs: costs.into_boxed_slice(),
            present: present.into_boxed_slice(),
            entries: entries.into_boxed_slice(),
            overflow,
        }
    }

    #[inline(always)]
    fn get(&self, pos1: Coordinates, pos2: Coordinates) -> Option<u8> {
        if let Some(slot) = mud_slot(self.width, self.height, pos1, pos2) {
            if self.costs.is_empty() || self.present[slot / 64] & (1 << (slot % 64)) == 0 {
                return None;
            }
            return Some(self.costs[slot]);
        }
        self.overflow.get(&(pos1, pos2)).copied()
    }

    fn to_mutable(&self) -> HashMap<MudKey, u8> {
        let mut mutable = self.overflow.clone();
        for &((pos1, pos2), value) in &self.entries {
            mutable.insert((pos1, pos2), value);
            mutable.insert((pos2, pos1), value);
        }
        mutable
    }
}

#[inline(always)]
fn mud_slot(width: u8, height: u8, pos1: Coordinates, pos2: Coordinates) -> Option<usize> {
    if pos1.x >= width || pos1.y >= height || pos2.x >= width || pos2.y >= height {
        return None;
    }

    let direction = if pos1.x == pos2.x {
        if pos1.y.checked_add(1) == Some(pos2.y) {
            0
        } else if pos2.y.checked_add(1) == Some(pos1.y) {
            2
        } else {
            return None;
        }
    } else if pos1.y == pos2.y {
        if pos1.x.checked_add(1) == Some(pos2.x) {
            1
        } else if pos2.x.checked_add(1) == Some(pos1.x) {
            3
        } else {
            return None;
        }
    } else {
        return None;
    };

    Some(pos1.to_index(width) * 4 + direction)
}

impl MudMap {
    pub fn new() -> Self {
        Self {
            storage: MudStorage::Mutable(HashMap::new()),
        }
    }

    pub(crate) fn freeze(self, width: u8, height: u8) -> Self {
        match self.storage {
            MudStorage::Mutable(mutable) => Self {
                storage: MudStorage::Frozen(Arc::new(FrozenMudMap::from_mutable(
                    width, height, mutable,
                ))),
            },
            MudStorage::Frozen(frozen) if frozen.width == width && frozen.height == height => {
                Self {
                    storage: MudStorage::Frozen(frozen),
                }
            },
            MudStorage::Frozen(frozen) => Self {
                storage: MudStorage::Frozen(Arc::new(FrozenMudMap::from_mutable(
                    width,
                    height,
                    frozen.to_mutable(),
                ))),
            },
        }
    }

    fn mutable(&mut self) -> &mut HashMap<MudKey, u8> {
        if let MudStorage::Frozen(frozen) = &self.storage {
            self.storage = MudStorage::Mutable(frozen.to_mutable());
        }
        let MudStorage::Mutable(mutable) = &mut self.storage else {
            unreachable!("frozen mud map was thawed")
        };
        mutable
    }

    /// Insert mud between two positions (order doesn't matter)
    pub fn insert(&mut self, pos1: Coordinates, pos2: Coordinates, value: u8) {
        let mutable = self.mutable();
        mutable.insert((pos1, pos2), value);
        mutable.insert((pos2, pos1), value);
    }

    /// Get mud value between two positions (order doesn't matter)
    #[inline(always)]
    pub fn get(&self, pos1: Coordinates, pos2: Coordinates) -> Option<u8> {
        match &self.storage {
            MudStorage::Mutable(mutable) => mutable.get(&(pos1, pos2)).copied(),
            MudStorage::Frozen(frozen) => frozen.get(pos1, pos2),
        }
    }

    /// Returns an iterator over all unique mud positions and their values
    pub fn iter(&self) -> impl Iterator<Item = MudEntry> + '_ {
        MudMapIter {
            inner: match &self.storage {
                MudStorage::Mutable(mutable) => MudMapIterInner::Mutable(mutable.iter()),
                MudStorage::Frozen(frozen) => MudMapIterInner::Frozen(frozen.entries.iter()),
            },
        }
    }

    /// Clear all mud
    pub fn clear(&mut self) {
        self.storage = MudStorage::Mutable(HashMap::new());
    }

    /// Returns the number of unique mud positions
    pub fn len(&self) -> usize {
        match &self.storage {
            MudStorage::Mutable(mutable) => mutable.len() / 2,
            MudStorage::Frozen(frozen) => frozen.entries.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.storage {
            MudStorage::Mutable(mutable) => mutable.is_empty(),
            MudStorage::Frozen(frozen) => frozen.entries.is_empty() && frozen.overflow.is_empty(),
        }
    }

    /// Check if mud exists between two positions (order doesn't matter)
    #[inline(always)]
    pub fn contains(&self, pos1: Coordinates, pos2: Coordinates) -> bool {
        self.get(pos1, pos2).is_some()
    }

    #[cfg(test)]
    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        match (&self.storage, &other.storage) {
            (MudStorage::Frozen(left), MudStorage::Frozen(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }
}

impl Default for MudMap {
    fn default() -> Self {
        Self::new()
    }
}

// Types for Python integration
#[cfg(feature = "python")]
#[derive(FromPyObject)]
pub enum CoordinatesInput {
    Tuple(i32, i32),
    Object(Coordinates),
}

/// Input type for Coordinates addition operations.
/// Accepts tuple deltas or Direction values (not other Coordinates).
#[cfg(feature = "python")]
#[derive(FromPyObject)]
pub enum CoordinatesAddInput {
    /// Tuple delta (dx, dy) - can be negative
    Tuple(i32, i32),
    /// Direction value (0-4: UP, RIGHT, DOWN, LEFT, STAY)
    Direction(u8),
}

#[cfg(feature = "python")]
impl From<CoordinatesInput> for PyResult<Coordinates> {
    fn from(input: CoordinatesInput) -> PyResult<Coordinates> {
        match input {
            CoordinatesInput::Tuple(x, y) => Coordinates::py_new(x, y),
            CoordinatesInput::Object(coords) => Ok(coords),
        }
    }
}

// Wall type
#[cfg_attr(
    feature = "python",
    pyclass(module = "pyrat_engine._core.types", frozen, get_all)
)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Wall {
    pub pos1: Coordinates,
    pub pos2: Coordinates,
}

#[cfg(feature = "python")]
#[pymethods]
impl Wall {
    #[new]
    fn new(pos1: CoordinatesInput, pos2: CoordinatesInput) -> PyResult<Self> {
        let coord1: Coordinates = PyResult::<Coordinates>::from(pos1)?;
        let coord2: Coordinates = PyResult::<Coordinates>::from(pos2)?;

        if !coord1.is_adjacent_to(&coord2) {
            return Err(PyValueError::new_err(format!(
                "Wall positions must be adjacent: {} and {}",
                coord1.__str__(),
                coord2.__str__()
            )));
        }

        // Normalize order (smaller position first)
        let (p1, p2) = if coord1 < coord2 {
            (coord1, coord2)
        } else {
            (coord2, coord1)
        };
        Ok(Self { pos1: p1, pos2: p2 })
    }

    fn blocks_movement(&self, from: Coordinates, to: Coordinates) -> bool {
        // Check if this wall blocks movement from one position to another
        if !from.is_adjacent_to(&to) {
            return false; // Can't directly move between non-adjacent positions
        }

        // Check if the wall is between these positions
        (self.pos1 == from && self.pos2 == to) || (self.pos1 == to && self.pos2 == from)
    }

    fn __repr__(&self) -> String {
        format!("Wall({}, {})", self.pos1.__repr__(), self.pos2.__repr__())
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.pos1 == other.pos1 && self.pos2 == other.pos2
    }

    fn __ne__(&self, other: &Self) -> bool {
        !self.__eq__(other)
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyIterator>> {
        let tuple = PyTuple::new(py, [self.pos1, self.pos2])?;
        pyo3::types::PyIterator::from_object(&tuple)
    }

    fn __len__(&self) -> usize {
        2
    }
}

// Mud type
#[cfg_attr(
    feature = "python",
    pyclass(module = "pyrat_engine._core.types", frozen, get_all)
)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Mud {
    pub pos1: Coordinates,
    pub pos2: Coordinates,
    pub value: u8,
}

#[cfg(feature = "python")]
#[pymethods]
impl Mud {
    #[new]
    fn new(pos1: CoordinatesInput, pos2: CoordinatesInput, value: u8) -> PyResult<Self> {
        let coord1: Coordinates = PyResult::<Coordinates>::from(pos1)?;
        let coord2: Coordinates = PyResult::<Coordinates>::from(pos2)?;

        if !coord1.is_adjacent_to(&coord2) {
            return Err(PyValueError::new_err(format!(
                "Mud positions must be adjacent: {} and {}",
                coord1.__str__(),
                coord2.__str__()
            )));
        }
        if value < 2 {
            return Err(PyValueError::new_err("Mud value must be at least 2"));
        }

        // Normalize order
        let (p1, p2) = if coord1 < coord2 {
            (coord1, coord2)
        } else {
            (coord2, coord1)
        };
        Ok(Self {
            pos1: p1,
            pos2: p2,
            value,
        })
    }

    fn affects_movement(&self, from: Coordinates, to: Coordinates) -> bool {
        self.blocks_movement(from, to)
    }

    fn blocks_movement(&self, from: Coordinates, to: Coordinates) -> bool {
        (self.pos1 == from && self.pos2 == to) || (self.pos1 == to && self.pos2 == from)
    }

    fn __repr__(&self) -> String {
        format!(
            "Mud({}, {}, value={})",
            self.pos1.__repr__(),
            self.pos2.__repr__(),
            self.value
        )
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.pos1 == other.pos1 && self.pos2 == other.pos2 && self.value == other.value
    }

    fn __ne__(&self, other: &Self) -> bool {
        !self.__eq__(other)
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyIterator>> {
        let pos1 = self.pos1.into_pyobject(py)?;
        let pos2 = self.pos2.into_pyobject(py)?;
        let value = self.value.into_pyobject(py)?;
        let tuple = PyTuple::new(py, [pos1.as_any(), pos2.as_any(), value.as_any()])?;
        pyo3::types::PyIterator::from_object(&tuple)
    }

    fn __len__(&self) -> usize {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mud_map() {
        let mut mud_map = MudMap::new();
        let pos1 = Coordinates::new(0, 0);
        let pos2 = Coordinates::new(0, 1);

        mud_map.insert(pos1, pos2, 2);

        // Inserting once establishes the bidirectional invariant.
        assert_eq!(mud_map.get(pos1, pos2), Some(2));
        assert_eq!(mud_map.get(pos2, pos1), Some(2));
        assert!(mud_map.contains(pos1, pos2));
        assert!(mud_map.contains(pos2, pos1));
        assert_eq!(mud_map.len(), 1);
        assert_eq!(mud_map.iter().collect::<Vec<_>>(), vec![((pos1, pos2), 2)]);

        // Reversed insertion updates the same logical edge in both directions.
        mud_map.insert(pos2, pos1, 3);
        assert_eq!(mud_map.get(pos1, pos2), Some(3));
        assert_eq!(mud_map.get(pos2, pos1), Some(3));
        assert_eq!(mud_map.len(), 1);
        assert_eq!(mud_map.iter().collect::<Vec<_>>(), vec![((pos1, pos2), 3)]);

        // Test non-existent mud
        assert_eq!(mud_map.get(pos1, Coordinates::new(1, 0)), None);
        assert!(!mud_map.contains(pos1, Coordinates::new(1, 0)));
    }

    #[test]
    fn frozen_mud_map_preserves_dense_and_overflow_entries() {
        let mut mud_map = MudMap::new();
        let vertical = (Coordinates::new(0, 0), Coordinates::new(0, 1));
        let horizontal = (Coordinates::new(1, 1), Coordinates::new(2, 1));
        let outside = (Coordinates::new(250, 250), Coordinates::new(251, 250));
        let self_edge = Coordinates::new(2, 2);
        mud_map.insert(vertical.0, vertical.1, 0);
        mud_map.insert(horizontal.0, horizontal.1, u8::MAX);
        mud_map.insert(outside.0, outside.1, 7);
        mud_map.insert(self_edge, self_edge, 9);

        let mut expected_entries: Vec<_> = mud_map.iter().collect();
        expected_entries.sort_unstable();
        let frozen = mud_map.freeze(3, 3);

        assert_eq!(frozen.get(vertical.0, vertical.1), Some(0));
        assert_eq!(frozen.get(vertical.1, vertical.0), Some(0));
        assert_eq!(frozen.get(horizontal.0, horizontal.1), Some(u8::MAX));
        assert_eq!(frozen.get(outside.0, outside.1), Some(7));
        assert_eq!(frozen.get(outside.1, outside.0), Some(7));
        assert_eq!(frozen.get(self_edge, self_edge), Some(9));
        assert_eq!(frozen.iter().collect::<Vec<_>>(), expected_entries);
        assert_eq!(frozen.len(), expected_entries.len());
        assert!(!frozen.is_empty());
    }

    #[test]
    fn frozen_mud_lookup_matches_mutable_map_for_every_cell_pair() {
        let width = 5;
        let height = 4;
        let mut mutable = MudMap::new();
        for y in 0..height {
            for x in 0..width {
                let position = Coordinates::new(x, y);
                if x + 1 < width {
                    mutable.insert(position, Coordinates::new(x + 1, y), x + y);
                }
                if y + 1 < height {
                    mutable.insert(position, Coordinates::new(x, y + 1), x + y + 1);
                }
            }
        }
        let expected = mutable.clone();
        let frozen = mutable.freeze(width, height);

        for first_y in 0..height {
            for first_x in 0..width {
                for second_y in 0..height {
                    for second_x in 0..width {
                        let first = Coordinates::new(first_x, first_y);
                        let second = Coordinates::new(second_x, second_y);
                        assert_eq!(frozen.get(first, second), expected.get(first, second));
                    }
                }
            }
        }
    }

    #[test]
    fn mutating_frozen_mud_map_thaws_only_that_clone() {
        let first = Coordinates::new(0, 0);
        let second = Coordinates::new(0, 1);
        let third = Coordinates::new(1, 1);
        let mut original = MudMap::new();
        original.insert(first, second, 2);
        let frozen = original.freeze(3, 3);
        let mut changed = frozen.clone();

        assert!(frozen.shares_storage_with(&changed));
        changed.insert(second, third, 3);

        assert!(!frozen.shares_storage_with(&changed));
        assert_eq!(frozen.get(first, second), Some(2));
        assert_eq!(frozen.get(second, third), None);
        assert_eq!(changed.get(first, second), Some(2));
        assert_eq!(changed.get(second, third), Some(3));

        changed.clear();
        assert!(changed.is_empty());
        assert_eq!(frozen.get(first, second), Some(2));
    }
    mod coordinates {
        use super::*;

        #[test]
        fn test_new_coordinates() {
            let coord = Coordinates::new(5, 10);
            assert_eq!(coord.x, 5);
            assert_eq!(coord.y, 10);
        }

        #[test]
        fn test_to_index() {
            let test_cases = [
                // (x, y, width, expected_index)
                (0, 0, 10, 0),  // Bottom-left corner (origin)
                (9, 0, 10, 9),  // Bottom-right corner
                (0, 9, 10, 90), // Top-left corner
                (9, 9, 10, 99), // Top-right corner
                (5, 5, 10, 55), // Middle
                (3, 2, 15, 33), // Non-square board
                (0, 1, 5, 5),   // Second row start
                (4, 1, 5, 9),   // Second row end
            ];

            for (x, y, width, expected) in test_cases {
                let coord = Coordinates::new(x, y);
                assert_eq!(
                    coord.to_index(width),
                    expected,
                    "Failed for x={x}, y={y}, width={width}"
                );
            }
        }

        #[test]
        fn test_coordinates_equality() {
            let coord1 = Coordinates::new(1, 2);
            let coord2 = Coordinates::new(1, 2);
            let coord3 = Coordinates::new(2, 1);

            assert_eq!(coord1, coord2);
            assert_ne!(coord1, coord3);
        }

        #[test]
        fn test_coordinates_clone() {
            let coord1 = Coordinates::new(1, 2);
            let coord2 = coord1;

            assert_eq!(coord1, coord2);
            // Ensure modifying one doesn't affect the other
            let coord3 = Coordinates::new(coord2.x + 1, coord2.y);
            assert_ne!(coord1, coord3);
        }
    }

    mod direction {
        use super::*;

        #[test]
        fn test_direction_apply_to() {
            let center = Coordinates::new(5, 5);

            // Test all directions from center with mathematical coordinate system
            assert_eq!(
                Direction::Up.apply_to(center),
                Coordinates::new(5, 6), // Moving up increases y
                "Up should increase y coordinate"
            );
            assert_eq!(
                Direction::Down.apply_to(center),
                Coordinates::new(5, 4), // Moving down decreases y
                "Down should decrease y coordinate"
            );
            assert_eq!(
                Direction::Left.apply_to(center),
                Coordinates::new(4, 5),
                "Left should decrease x coordinate"
            );
            assert_eq!(
                Direction::Right.apply_to(center),
                Coordinates::new(6, 5),
                "Right should increase x coordinate"
            );
            assert_eq!(Direction::Stay.apply_to(center), center);
        }

        #[test]
        fn test_coordinate_system_edges() {
            // Test bottom edge (y = 0)
            let bottom = Coordinates::new(5, 0);
            assert_eq!(
                Direction::Down.apply_to(bottom),
                Coordinates::new(5, 0),
                "Down at bottom edge should saturate"
            );
            assert_eq!(
                Direction::Up.apply_to(bottom),
                Coordinates::new(5, 1),
                "Up from bottom should increase y"
            );

            // Test top edge (y = 255)
            let top = Coordinates::new(5, 255);
            assert_eq!(
                Direction::Up.apply_to(top),
                Coordinates::new(5, 255),
                "Up at top edge should saturate"
            );
            assert_eq!(
                Direction::Down.apply_to(top),
                Coordinates::new(5, 254),
                "Down from top should decrease y"
            );

            // Test origin behavior
            let origin = Coordinates::new(0, 0); // Bottom-left corner
            assert_eq!(
                Direction::Down.apply_to(origin),
                Coordinates::new(0, 0),
                "Down from origin should stay at origin"
            );
            assert_eq!(
                Direction::Up.apply_to(origin),
                Coordinates::new(0, 1),
                "Up from origin should increase y"
            );
        }

        #[test]
        fn test_initial_positions() {
            // Test movements from player starting positions
            let player1_start = Coordinates::new(0, 9); // Top-right in a 10x10 grid
            let player2_start = Coordinates::new(9, 0); // Bottom-left in a 10x10 grid

            assert_eq!(
                Direction::Down.apply_to(player1_start),
                Coordinates::new(0, 8),
                "Player1 moving down should decrease y"
            );
            assert_eq!(
                Direction::Right.apply_to(player2_start),
                Coordinates::new(10, 0),
                "Player2 moving right should increase x"
            );
        }

        #[test]
        fn test_saturating_behavior() {
            // Tests that the positions get saturated correctly
            let bottom_left = Coordinates::new(0, 0);
            let upper_right = Coordinates::new(255, 255);

            assert_eq!(Direction::Up.apply_to(upper_right), upper_right);
            assert_eq!(Direction::Right.apply_to(upper_right), upper_right);
            assert_eq!(Direction::Left.apply_to(bottom_left), bottom_left);
            assert_eq!(Direction::Down.apply_to(bottom_left), bottom_left);
        }

        #[test]
        fn test_cardinals_excludes_stay() {
            assert_eq!(Direction::CARDINALS.len(), 4);
            assert!(!Direction::CARDINALS.contains(&Direction::Stay));
        }

        #[test]
        fn test_between_adjacent() {
            let a = Coordinates::new(1, 1);
            assert_eq!(
                Direction::between(a, Coordinates::new(1, 2)),
                Some(Direction::Up)
            );
            assert_eq!(
                Direction::between(a, Coordinates::new(1, 0)),
                Some(Direction::Down)
            );
            assert_eq!(
                Direction::between(a, Coordinates::new(2, 1)),
                Some(Direction::Right)
            );
            assert_eq!(
                Direction::between(a, Coordinates::new(0, 1)),
                Some(Direction::Left)
            );
        }

        #[test]
        fn test_between_non_adjacent() {
            let a = Coordinates::new(1, 1);
            assert_eq!(Direction::between(a, Coordinates::new(3, 1)), None);
            assert_eq!(Direction::between(a, Coordinates::new(2, 2)), None);
            assert_eq!(Direction::between(a, a), None);
        }

        #[test]
        fn test_direction_ordering() {
            assert_eq!(Direction::Up as u8, 0);
            assert_eq!(Direction::Right as u8, 1);
            assert_eq!(Direction::Down as u8, 2);
            assert_eq!(Direction::Left as u8, 3);
            assert_eq!(Direction::Stay as u8, 4);
        }

        #[test]
        fn test_direction_equality() {
            assert_eq!(Direction::Up, Direction::Up);
            assert_ne!(Direction::Up, Direction::Down);
            assert_ne!(Direction::Left, Direction::Right);
            assert_eq!(Direction::Stay, Direction::Stay);
        }

        #[test]
        fn test_all_directions_center() {
            let center = Coordinates::new(10, 10);
            let moves = [
                (Direction::Up, Coordinates::new(10, 11)),
                (Direction::Right, Coordinates::new(11, 10)),
                (Direction::Down, Coordinates::new(10, 9)),
                (Direction::Left, Coordinates::new(9, 10)),
                (Direction::Stay, center),
            ];

            for (direction, expected) in moves {
                assert_eq!(
                    direction.apply_to(center),
                    expected,
                    "Failed for direction {direction:?}"
                );
            }
        }
    }
}

#[cfg(all(test, feature = "python"))]
mod python_tests {
    use super::*;

    #[test]
    fn test_coordinates_python_constructor() {
        // Test py_new validation with negative values
        assert!(Coordinates::py_new(-1, 0).is_err());
        assert!(Coordinates::py_new(0, -1).is_err());

        // Test values too large
        assert!(Coordinates::py_new(256, 0).is_err());
        assert!(Coordinates::py_new(0, 256).is_err());

        // Test valid construction
        let coords = Coordinates::py_new(5, 10).unwrap();
        assert_eq!(coords.x, 5);
        assert_eq!(coords.y, 10);
    }

    #[test]
    fn test_get_neighbor() {
        let pos = Coordinates::new(5, 5);

        // Test all directions
        assert_eq!(pos.get_neighbor(0).unwrap(), Coordinates::new(5, 6)); // UP
        assert_eq!(pos.get_neighbor(1).unwrap(), Coordinates::new(6, 5)); // RIGHT
        assert_eq!(pos.get_neighbor(2).unwrap(), Coordinates::new(5, 4)); // DOWN
        assert_eq!(pos.get_neighbor(3).unwrap(), Coordinates::new(4, 5)); // LEFT
        assert_eq!(pos.get_neighbor(4).unwrap(), Coordinates::new(5, 5)); // STAY

        // Test invalid direction
        assert!(pos.get_neighbor(5).is_err());
    }

    #[test]
    fn test_is_adjacent_to() {
        let pos1 = Coordinates::new(5, 5);

        // Adjacent positions
        assert!(pos1.is_adjacent_to(&Coordinates::new(5, 6))); // Up
        assert!(pos1.is_adjacent_to(&Coordinates::new(5, 4))); // Down
        assert!(pos1.is_adjacent_to(&Coordinates::new(4, 5))); // Left
        assert!(pos1.is_adjacent_to(&Coordinates::new(6, 5))); // Right

        // Non-adjacent positions
        assert!(!pos1.is_adjacent_to(&Coordinates::new(5, 5))); // Same position
        assert!(!pos1.is_adjacent_to(&Coordinates::new(6, 6))); // Diagonal
        assert!(!pos1.is_adjacent_to(&Coordinates::new(7, 5))); // Too far
    }

    #[test]
    fn test_manhattan_distance() {
        let pos1 = Coordinates::new(0, 0);
        let pos2 = Coordinates::new(3, 4);

        assert_eq!(pos1.manhattan_distance(&pos2), 7);
        assert_eq!(pos2.manhattan_distance(&pos1), 7); // Symmetric

        // Same position
        assert_eq!(pos1.manhattan_distance(&pos1), 0);
    }

    #[test]
    fn test_string_representations() {
        let pos = Coordinates::new(5, 10);

        assert_eq!(pos.__repr__(), "Coordinates(5, 10)");
        assert_eq!(pos.__str__(), "(5, 10)");
    }

    #[test]
    fn test_python_indexing() {
        let pos = Coordinates::new(5, 10);
        // Test using __getitem__ which is what Python uses for unpacking
        assert_eq!(pos.__getitem__(0).unwrap(), 5);
        assert_eq!(pos.__getitem__(1).unwrap(), 10);

        // Test negative indexing
        assert_eq!(pos.__getitem__(-2).unwrap(), 5);
        assert_eq!(pos.__getitem__(-1).unwrap(), 10);

        // Test out of bounds
        assert!(pos.__getitem__(2).is_err());
        assert!(pos.__getitem__(-3).is_err());
    }

    // Tests for CoordinatesInput enum
    #[test]
    fn test_coordinates_input_from_tuple() {
        let input = CoordinatesInput::Tuple(5, 10);
        let coords: PyResult<Coordinates> = input.into();
        assert!(coords.is_ok());
        assert_eq!(coords.unwrap(), Coordinates::new(5, 10));
    }

    #[test]
    fn test_coordinates_input_from_object() {
        let coords = Coordinates::new(5, 10);
        let input = CoordinatesInput::Object(coords);
        let result: PyResult<Coordinates> = input.into();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), coords);
    }

    #[test]
    fn test_coordinates_input_validation() {
        // Test invalid tuple
        let input = CoordinatesInput::Tuple(-1, 10);
        let coords: PyResult<Coordinates> = input.into();
        assert!(coords.is_err());
    }

    // Tests for Wall type
    #[test]
    fn test_wall_creation() {
        let pos1 = Coordinates::new(0, 0);
        let pos2 = Coordinates::new(0, 1);

        let wall = Wall::new(
            CoordinatesInput::Object(pos1),
            CoordinatesInput::Object(pos2),
        );
        assert!(wall.is_ok());
        let wall = wall.unwrap();
        assert_eq!(wall.pos1, pos1);
        assert_eq!(wall.pos2, pos2);
    }

    #[test]
    fn test_wall_validation() {
        // Non-adjacent positions
        let pos1 = Coordinates::new(0, 0);
        let pos2 = Coordinates::new(2, 2);

        let wall = Wall::new(
            CoordinatesInput::Object(pos1),
            CoordinatesInput::Object(pos2),
        );
        assert!(wall.is_err());
    }

    #[test]
    fn test_wall_normalization() {
        // Order should be normalized (smaller position first)
        let wall1 = Wall::new(
            CoordinatesInput::Object(Coordinates::new(1, 0)),
            CoordinatesInput::Object(Coordinates::new(0, 0)),
        )
        .unwrap();
        let wall2 = Wall::new(
            CoordinatesInput::Object(Coordinates::new(0, 0)),
            CoordinatesInput::Object(Coordinates::new(1, 0)),
        )
        .unwrap();
        assert_eq!(wall1.pos1, wall2.pos1);
        assert_eq!(wall1.pos2, wall2.pos2);
    }

    #[test]
    fn test_wall_blocks_movement() {
        let wall = Wall::new(
            CoordinatesInput::Object(Coordinates::new(0, 0)),
            CoordinatesInput::Object(Coordinates::new(0, 1)),
        )
        .unwrap();

        // Should block movement between the two positions
        assert!(wall.blocks_movement(Coordinates::new(0, 0), Coordinates::new(0, 1)));
        assert!(wall.blocks_movement(Coordinates::new(0, 1), Coordinates::new(0, 0)));

        // Should not block unrelated movements
        assert!(!wall.blocks_movement(Coordinates::new(1, 0), Coordinates::new(1, 1)));
        assert!(!wall.blocks_movement(Coordinates::new(0, 0), Coordinates::new(1, 0)));
    }

    // Tests for Mud type
    #[test]
    fn test_mud_creation() {
        let pos1 = Coordinates::new(0, 0);
        let pos2 = Coordinates::new(0, 1);

        let mud = Mud::new(
            CoordinatesInput::Object(pos1),
            CoordinatesInput::Object(pos2),
            3,
        );
        assert!(mud.is_ok());
        let mud = mud.unwrap();
        assert_eq!(mud.pos1, pos1);
        assert_eq!(mud.pos2, pos2);
        assert_eq!(mud.value, 3);
    }

    #[test]
    fn test_mud_validation() {
        let pos1 = Coordinates::new(0, 0);
        let pos2 = Coordinates::new(0, 1);

        // Mud value too low
        assert!(Mud::new(
            CoordinatesInput::Object(pos1),
            CoordinatesInput::Object(pos2),
            1
        )
        .is_err());

        // Non-adjacent positions
        let pos3 = Coordinates::new(2, 2);
        assert!(Mud::new(
            CoordinatesInput::Object(pos1),
            CoordinatesInput::Object(pos3),
            3
        )
        .is_err());
    }

    // Tests for Coordinates arithmetic operations
    #[test]
    fn test_coordinates_add_tuple_positive() {
        let pos = Coordinates::new(5, 5);

        // Positive deltas
        let result = pos.__add__(CoordinatesAddInput::Tuple(2, 3)).unwrap();
        assert_eq!(result, Coordinates::new(7, 8));
    }

    #[test]
    fn test_coordinates_add_tuple_negative() {
        let pos = Coordinates::new(5, 5);

        // Negative deltas
        let result = pos.__add__(CoordinatesAddInput::Tuple(-2, -3)).unwrap();
        assert_eq!(result, Coordinates::new(3, 2));
    }

    #[test]
    fn test_coordinates_add_tuple_saturation() {
        // Saturation at 0
        let pos = Coordinates::new(5, 5);
        let result = pos.__add__(CoordinatesAddInput::Tuple(-10, -10)).unwrap();
        assert_eq!(result, Coordinates::new(0, 0));

        // Saturation at 255
        let pos = Coordinates::new(250, 250);
        let result = pos.__add__(CoordinatesAddInput::Tuple(10, 10)).unwrap();
        assert_eq!(result, Coordinates::new(255, 255));
    }

    #[test]
    fn test_coordinates_add_direction() {
        let pos = Coordinates::new(5, 5);

        // UP (0)
        let result = pos.__add__(CoordinatesAddInput::Direction(0)).unwrap();
        assert_eq!(result, Coordinates::new(5, 6));

        // RIGHT (1)
        let result = pos.__add__(CoordinatesAddInput::Direction(1)).unwrap();
        assert_eq!(result, Coordinates::new(6, 5));

        // DOWN (2)
        let result = pos.__add__(CoordinatesAddInput::Direction(2)).unwrap();
        assert_eq!(result, Coordinates::new(5, 4));

        // LEFT (3)
        let result = pos.__add__(CoordinatesAddInput::Direction(3)).unwrap();
        assert_eq!(result, Coordinates::new(4, 5));

        // STAY (4)
        let result = pos.__add__(CoordinatesAddInput::Direction(4)).unwrap();
        assert_eq!(result, Coordinates::new(5, 5));
    }

    #[test]
    fn test_coordinates_add_direction_invalid() {
        let pos = Coordinates::new(5, 5);

        // Invalid direction (5)
        let result = pos.__add__(CoordinatesAddInput::Direction(5));
        assert!(result.is_err());
    }

    #[test]
    fn test_coordinates_sub() {
        let pos1 = Coordinates::new(5, 5);
        let pos2 = Coordinates::new(3, 8);

        // Subtraction returns signed delta
        let delta = pos1.__sub__(CoordinatesInput::Object(pos2)).unwrap();
        assert_eq!(delta, (2, -3));

        // Reverse subtraction
        let delta = pos2.__sub__(CoordinatesInput::Object(pos1)).unwrap();
        assert_eq!(delta, (-2, 3));
    }

    #[test]
    fn test_coordinates_sub_tuple_input() {
        let pos = Coordinates::new(5, 5);

        // Subtraction with tuple input
        let delta = pos.__sub__(CoordinatesInput::Tuple(3, 8)).unwrap();
        assert_eq!(delta, (2, -3));
    }
}
