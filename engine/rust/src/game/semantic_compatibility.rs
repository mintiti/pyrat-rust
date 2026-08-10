use std::collections::HashMap;

use crate::{Coordinates, Direction, GameBuilder, GameState, MazeParams, MudMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    first: Coordinates,
    second: Coordinates,
}

impl Edge {
    fn new(first: Coordinates, second: Coordinates) -> Self {
        if first <= second {
            Self { first, second }
        } else {
            Self {
                first: second,
                second: first,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct MudEdge {
    edge: Edge,
    cost: u8,
}

#[derive(Debug, PartialEq, Eq)]
struct TopologyFingerprint {
    width: u8,
    height: u8,
    walls: Vec<Edge>,
    mud: Vec<MudEdge>,
}

impl TopologyFingerprint {
    fn from_game(game: &GameState) -> Self {
        let mut walls: Vec<_> = game
            .wall_entries()
            .into_iter()
            .map(|wall| Edge::new(wall.pos1, wall.pos2))
            .collect();
        walls.sort_unstable();
        walls.dedup();

        let mut mud: Vec<_> = game
            .mud_positions()
            .iter()
            .map(|((first, second), cost)| MudEdge {
                edge: Edge::new(first, second),
                cost,
            })
            .collect();
        mud.sort_unstable();
        mud.dedup();

        Self {
            width: game.width(),
            height: game.height(),
            walls,
            mud,
        }
    }

    /// Canonical, representation-independent encoding used only by these
    /// compatibility fixtures.
    ///
    /// Layout: dimensions, little-endian wall count, wall endpoints,
    /// little-endian mud count, then mud endpoints and traversal costs.
    fn canonical_bytes(&self) -> Vec<u8> {
        let wall_count = u16::try_from(self.walls.len()).expect("wall fixture count fits in u16");
        let mud_count = u16::try_from(self.mud.len()).expect("mud fixture count fits in u16");
        let mut bytes = Vec::with_capacity(6 + self.walls.len() * 4 + self.mud.len() * 5);

        bytes.extend([self.width, self.height]);
        bytes.extend(wall_count.to_le_bytes());
        for wall in &self.walls {
            bytes.extend([wall.first.x, wall.first.y, wall.second.x, wall.second.y]);
        }
        bytes.extend(mud_count.to_le_bytes());
        for mud in &self.mud {
            bytes.extend([
                mud.edge.first.x,
                mud.edge.first.y,
                mud.edge.second.x,
                mud.edge.second.y,
                mud.cost,
            ]);
        }

        bytes
    }
}

fn coordinate(x: u8, y: u8) -> Coordinates {
    Coordinates::new(x, y)
}

fn open_game(seed: u64) -> GameState {
    GameBuilder::new(3, 3)
        .with_max_turns(300)
        .with_open_maze()
        .with_corner_positions()
        .with_custom_cheese(vec![
            coordinate(1, 0),
            coordinate(1, 1),
            coordinate(1, 2),
            coordinate(0, 2),
            coordinate(2, 0),
        ])
        .build()
        .create(Some(seed))
        .expect("open compatibility fixture should be valid")
}

fn add_wall(
    walls: &mut HashMap<Coordinates, Vec<Coordinates>>,
    first: Coordinates,
    second: Coordinates,
) {
    walls.entry(first).or_default().push(second);
    walls.entry(second).or_default().push(first);
}

fn fixed_game(seed: Option<u64>) -> GameState {
    let mut walls = HashMap::new();
    // Intentionally insert some endpoints in reverse canonical order. The
    // fingerprint describes the resulting topology, not input-map ordering.
    add_wall(&mut walls, coordinate(1, 0), coordinate(0, 0));
    add_wall(&mut walls, coordinate(1, 1), coordinate(2, 1));
    add_wall(&mut walls, coordinate(3, 2), coordinate(3, 1));

    let mut mud = MudMap::new();
    mud.insert(coordinate(0, 1), coordinate(0, 0), 2);
    mud.insert(coordinate(3, 2), coordinate(2, 2), 3);

    GameBuilder::new(4, 3)
        .with_max_turns(300)
        .with_custom_maze(walls, mud)
        .with_corner_positions()
        .with_custom_cheese(vec![
            coordinate(0, 1),
            coordinate(1, 1),
            coordinate(2, 2),
            coordinate(3, 1),
            coordinate(1, 2),
        ])
        .build()
        .create(seed)
        .expect("fixed compatibility fixture should be valid")
}

fn seeded_disconnected_game(seed: u64) -> GameState {
    GameBuilder::new(3, 3)
        .with_random_maze(MazeParams {
            wall_density: 0.55,
            connected: false,
            symmetric: true,
            mud_density: 0.65,
            mud_range: 4,
        })
        .with_corner_positions()
        .with_custom_cheese(vec![coordinate(1, 1)])
        .build()
        .create(Some(seed))
        .expect("seeded disconnected compatibility fixture should be valid")
}

fn seeded_connected_classic_game(seed: u64) -> GameState {
    GameBuilder::new(7, 5)
        .with_random_maze(MazeParams::classic())
        .with_corner_positions()
        .with_custom_cheese(vec![coordinate(3, 2)])
        .build()
        .create(Some(seed))
        .expect("seeded connected compatibility fixture should be valid")
}

fn seeded_symmetric_border_repair_game(seed: u64) -> GameState {
    GameBuilder::new(3, 3)
        .with_random_maze(MazeParams {
            wall_density: 1.0,
            connected: false,
            symmetric: true,
            mud_density: 1.0,
            mud_range: 4,
        })
        .with_corner_positions()
        .with_custom_cheese(vec![coordinate(1, 1)])
        .build()
        .create(Some(seed))
        .expect("seeded border-repair compatibility fixture should be valid")
}

#[test]
fn seeded_disconnected_topology_and_state_hash_are_stable() {
    let game = seeded_disconnected_game(0xA11C_E5E5);
    let topology = TopologyFingerprint::from_game(&game);

    assert_eq!(
        topology.canonical_bytes(),
        vec![
            0x03, 0x03, 0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00, 0x01,
            0x01, 0x01, 0x01, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x02, 0x01,
            0x01, 0x02, 0x02, 0x02, 0x02, 0x00, 0x02, 0x01, 0x04, 0x00, 0x00, 0x00, 0x00, 0x01,
            0x02, 0x00, 0x02, 0x01, 0x02, 0x02, 0x01, 0x00, 0x02, 0x00, 0x02, 0x02, 0x01, 0x02,
            0x02, 0x02,
        ]
    );
    assert_eq!(game.state_hash(), 0x8177_5E20_C1DE_7EE2);
}

#[test]
fn seeded_connected_classic_topology_and_state_hash_are_stable() {
    let game = seeded_connected_classic_game(0xD15C_01A7);
    let topology = TopologyFingerprint::from_game(&game);

    assert_eq!(
        topology.canonical_bytes(),
        vec![
            7, 5, 24, 0, 0, 2, 0, 3, 0, 3, 0, 4, 1, 0, 1, 1, 1, 0, 2, 0, 1, 1, 1, 2, 1, 1, 2, 1, 1,
            3, 2, 3, 1, 4, 2, 4, 2, 2, 2, 3, 2, 2, 3, 2, 2, 3, 2, 4, 3, 0, 3, 1, 3, 2, 4, 2, 3, 3,
            3, 4, 4, 0, 4, 1, 4, 0, 5, 0, 4, 1, 4, 2, 4, 1, 5, 1, 4, 3, 5, 3, 4, 4, 5, 4, 5, 2, 5,
            3, 5, 3, 5, 4, 6, 0, 6, 1, 6, 1, 6, 2, 2, 0, 2, 4, 3, 4, 3, 3, 0, 4, 0, 3,
        ]
    );
    assert_eq!(game.state_hash(), 0x4766_48D1_8D5C_2B13);
}

#[test]
fn seeded_symmetric_border_repair_topology_and_state_hash_are_stable() {
    let game = seeded_symmetric_border_repair_game(0xB04D_EA5E);
    let topology = TopologyFingerprint::from_game(&game);

    assert_eq!(
        topology.canonical_bytes(),
        vec![
            3, 3, 6, 0, 0, 0, 0, 1, 0, 2, 1, 2, 1, 0, 1, 1, 1, 0, 2, 0, 1, 1, 1, 2, 2, 1, 2, 2, 6,
            0, 0, 0, 1, 0, 2, 0, 1, 0, 2, 2, 0, 1, 1, 1, 3, 1, 1, 2, 1, 3, 1, 2, 2, 2, 2, 2, 0, 2,
            1, 2,
        ]
    );
    assert_eq!(game.state_hash(), 0xE511_3619_2A54_EA01);
}

#[test]
fn open_topology_and_state_hash_trace_are_stable() {
    let mut game = open_game(0);
    let independently_created = open_game(999);
    let expected_topology = TopologyFingerprint {
        width: 3,
        height: 3,
        walls: vec![],
        mud: vec![],
    };

    assert_eq!(TopologyFingerprint::from_game(&game), expected_topology);
    assert_eq!(
        TopologyFingerprint::from_game(&independently_created),
        expected_topology
    );
    assert_eq!(
        expected_topology.canonical_bytes(),
        vec![0x03, 0x03, 0x00, 0x00, 0x00, 0x00]
    );

    assert_eq!(game.state_hash(), 0xEECE_2E0B_481F_A67D);
    assert_eq!(independently_created.state_hash(), 0xEECE_2E0B_481F_A67D);

    let trace = [
        (Direction::Right, Direction::Left, 0xEFAF_A981_EBCE_3195),
        (Direction::Up, Direction::Down, 0xBAF9_7F69_B1EB_FBA3),
    ];
    for (turn, (player1, player2, expected_hash)) in trace.into_iter().enumerate() {
        let result = game.process_turn(player1, player2);
        assert!(!result.game_over, "open fixture ended on turn {}", turn + 1);
        assert_eq!(
            game.state_hash(),
            expected_hash,
            "open fixture hash changed on turn {}",
            turn + 1
        );
    }
}

#[test]
fn fixed_topology_and_state_hash_trace_are_stable() {
    let mut game = fixed_game(Some(0));
    let independently_created = fixed_game(Some(999));
    let expected_topology = TopologyFingerprint {
        width: 4,
        height: 3,
        walls: vec![
            Edge::new(coordinate(0, 0), coordinate(1, 0)),
            Edge::new(coordinate(1, 1), coordinate(2, 1)),
            Edge::new(coordinate(3, 1), coordinate(3, 2)),
        ],
        mud: vec![
            MudEdge {
                edge: Edge::new(coordinate(0, 0), coordinate(0, 1)),
                cost: 2,
            },
            MudEdge {
                edge: Edge::new(coordinate(2, 2), coordinate(3, 2)),
                cost: 3,
            },
        ],
    };

    assert_eq!(TopologyFingerprint::from_game(&game), expected_topology);
    assert_eq!(
        TopologyFingerprint::from_game(&independently_created),
        expected_topology
    );
    assert_eq!(
        expected_topology.canonical_bytes(),
        vec![
            0x04, 0x03, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x01, 0x02, 0x01, 0x03, 0x01,
            0x03, 0x02, 0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02, 0x02, 0x03, 0x02, 0x03,
        ]
    );

    assert_eq!(game.state_hash(), 0xDA23_7467_8CB6_02A2);
    assert_eq!(independently_created.state_hash(), 0xDA23_7467_8CB6_02A2);

    let trace = [
        (Direction::Up, Direction::Left, 0xB3B4_19A2_A88C_BD1D),
        (Direction::Right, Direction::Down, 0x9E48_5CCC_50E5_B2C1),
        (Direction::Stay, Direction::Stay, 0x853B_6C9A_BDCB_A778),
        (Direction::Right, Direction::Stay, 0x845C_F2B9_38C9_0D41),
    ];
    for (turn, (player1, player2, expected_hash)) in trace.into_iter().enumerate() {
        let result = game.process_turn(player1, player2);
        assert!(
            !result.game_over,
            "fixed fixture ended on turn {}",
            turn + 1
        );
        assert_eq!(
            game.state_hash(),
            expected_hash,
            "fixed fixture hash changed on turn {}",
            turn + 1
        );
    }
}
