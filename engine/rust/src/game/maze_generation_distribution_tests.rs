use super::*;
use crate::{GameBuilder, GameState, MazeParams};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::process::Command;

const REPORT_PREFIX: &str = "PYRAT_PR9_REPORT";
const TOPOLOGY_PREFIX: &str = "PYRAT_PR9_TOPOLOGY";
const REPORT_PROCESSES: usize = 8;

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

    fn is_horizontal(self) -> bool {
        self.first.y == self.second.y
    }
}

struct GeneratedSample {
    config: MazeConfig,
    initial_passages: BTreeSet<Edge>,
    final_passages: BTreeSet<Edge>,
    mud: BTreeMap<Edge, u8>,
}

impl GeneratedSample {
    fn repair_edges(&self) -> impl Iterator<Item = Edge> + '_ {
        self.final_passages
            .difference(&self.initial_passages)
            .copied()
    }

    fn repair_orbit_key(&self, edge: Edge) -> Edge {
        if !self.config.symmetry {
            return edge;
        }

        let mirror = Edge::new(
            symmetric(self.config, edge.first),
            symmetric(self.config, edge.second),
        );
        edge.min(mirror)
    }

    fn topology_bytes(&self) -> Vec<u8> {
        let passage_count =
            u32::try_from(self.final_passages.len()).expect("diagnostic passage count fits in u32");
        let mut bytes = Vec::with_capacity(6 + self.final_passages.len() * 4);

        bytes.extend([self.config.width, self.config.height]);
        bytes.extend(passage_count.to_le_bytes());
        for edge in &self.final_passages {
            bytes.extend([edge.first.x, edge.first.y, edge.second.x, edge.second.y]);
        }

        bytes
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        let mud_count = u32::try_from(self.mud.len()).expect("diagnostic mud count fits in u32");
        let mut bytes = self.topology_bytes();
        bytes.reserve(4 + self.mud.len() * 5);
        bytes.extend(mud_count.to_le_bytes());
        for (edge, cost) in &self.mud {
            bytes.extend([
                edge.first.x,
                edge.first.y,
                edge.second.x,
                edge.second.y,
                *cost,
            ]);
        }

        bytes
    }
}

#[derive(Default)]
struct CorpusSummary {
    seeds: usize,
    topology_bytes: BTreeSet<Vec<u8>>,
    topology_digest: u64,
    full_digest: u64,
    passages: u64,
    cycle_rank: u64,
    repair_edges: u64,
    repair_orbits: u64,
    muddy_repair_orbits: u64,
    horizontal_candidates: u64,
    horizontal_repairs: u64,
    vertical_candidates: u64,
    vertical_repairs: u64,
    dead_ends: u64,
    junctions: u64,
    corner_distance: u64,
    spatial_candidates: [u64; 16],
    spatial_repairs: [u64; 16],
}

impl CorpusSummary {
    fn add(&mut self, sample: &GeneratedSample) {
        let topology_bytes = sample.topology_bytes();
        self.topology_digest = fnv1a(self.topology_digest, &topology_bytes);
        self.topology_bytes.insert(topology_bytes);
        self.full_digest = fnv1a(self.full_digest, &sample.canonical_bytes());
        self.seeds += 1;

        let cell_count = usize::from(sample.config.width) * usize::from(sample.config.height);
        self.passages += sample.final_passages.len() as u64;
        self.cycle_rank += (sample.final_passages.len() + 1 - cell_count) as u64;

        let repair_edges: Vec<_> = sample.repair_edges().collect();
        let repair_orbits: BTreeSet<_> = repair_edges
            .iter()
            .map(|&edge| sample.repair_orbit_key(edge))
            .collect();
        let muddy_repair_orbits: BTreeSet<_> = repair_edges
            .iter()
            .filter(|edge| sample.mud.contains_key(edge))
            .map(|&edge| sample.repair_orbit_key(edge))
            .collect();
        let candidate_orbits: BTreeSet<_> = all_grid_edges(sample.config)
            .filter(|edge| !sample.initial_passages.contains(edge))
            .map(|edge| sample.repair_orbit_key(edge))
            .collect();

        self.repair_edges += repair_edges.len() as u64;
        self.repair_orbits += repair_orbits.len() as u64;
        self.muddy_repair_orbits += muddy_repair_orbits.len() as u64;

        for edge in candidate_orbits {
            if edge.is_horizontal() {
                self.horizontal_candidates += 1;
            } else {
                self.vertical_candidates += 1;
            }
            self.spatial_candidates[folded_spatial_bucket(sample.config, edge)] += 1;
        }
        for edge in repair_orbits.iter().copied() {
            if edge.is_horizontal() {
                self.horizontal_repairs += 1;
            } else {
                self.vertical_repairs += 1;
            }
            self.spatial_repairs[folded_spatial_bucket(sample.config, edge)] += 1;
        }

        let (dead_ends, junctions) = degree_counts(sample);
        self.dead_ends += dead_ends as u64;
        self.junctions += junctions as u64;
        self.corner_distance += corner_distance(sample) as u64;
    }

    fn report(&self, name: &str) -> String {
        let seeds = self.seeds as f64;
        let horizontal_rate = ratio(self.horizontal_repairs, self.horizontal_candidates);
        let vertical_rate = ratio(self.vertical_repairs, self.vertical_candidates);
        let spatial_rates = self
            .spatial_repairs
            .iter()
            .zip(self.spatial_candidates.iter())
            .map(|(&repairs, &candidates)| {
                if candidates == 0 {
                    "-".to_owned()
                } else {
                    format!("{:.5}", ratio(repairs, candidates))
                }
            })
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "{REPORT_PREFIX}\t{name}\tseeds={}\tunique_topologies={}\ttopology_digest={:016x}\tfull_digest={:016x}\tpassages_mean={:.5}\tcycle_mean={:.5}\trepair_edges_mean={:.5}\trepair_orbits_mean={:.5}\tmud_orbit_rate={:.6}\thorizontal_rate={horizontal_rate:.6}\tvertical_rate={vertical_rate:.6}\tdead_ends_mean={:.5}\tjunctions_mean={:.5}\tcorner_distance_mean={:.5}\tspatial_rates={spatial_rates}",
            self.seeds,
            self.topology_bytes.len(),
            self.topology_digest,
            self.full_digest,
            self.passages as f64 / seeds,
            self.cycle_rank as f64 / seeds,
            self.repair_edges as f64 / seeds,
            self.repair_orbits as f64 / seeds,
            ratio(self.muddy_repair_orbits, self.repair_orbits),
            self.dead_ends as f64 / seeds,
            self.junctions as f64 / seeds,
            self.corner_distance as f64 / seeds,
        )
    }
}

fn classic_config(width: u8, height: u8, seed: u64) -> MazeConfig {
    MazeConfig {
        width,
        height,
        target_density: 0.7,
        connected: true,
        symmetry: true,
        mud_density: 0.1,
        mud_range: 3,
        seed: Some(seed),
    }
}

fn generate_sample(config: MazeConfig) -> GeneratedSample {
    let mut generator = MazeGenerator::new(config);
    generator.generate_initial_layout();
    let initial_passages = connection_edges(&generator.connections);

    if config.connected {
        generator.repair_connectivity();
    }
    generator.add_border_connections();
    generator
        .validate_output()
        .expect("diagnostic generation must satisfy maze invariants");

    GeneratedSample {
        config,
        initial_passages,
        final_passages: connection_edges(&generator.connections),
        mud: generator
            .mud
            .iter()
            .map(|((first, second), cost)| (Edge::new(first, second), cost))
            .collect(),
    }
}

fn connection_edges(connections: &ConnectionGrid) -> BTreeSet<Edge> {
    let config = MazeConfig {
        width: connections.width,
        height: connections.height,
        target_density: 0.0,
        connected: false,
        symmetry: false,
        mud_density: 0.0,
        mud_range: 2,
        seed: None,
    };
    all_grid_edges(config)
        .filter(|edge| connections.contains(edge.first, edge.second))
        .collect()
}

fn all_grid_edges(config: MazeConfig) -> impl Iterator<Item = Edge> {
    (0..config.width).flat_map(move |x| {
        (0..config.height).flat_map(move |y| {
            let current = Coordinates::new(x, y);
            let right =
                (x + 1 < config.width).then(|| Edge::new(current, Coordinates::new(x + 1, y)));
            let up =
                (y + 1 < config.height).then(|| Edge::new(current, Coordinates::new(x, y + 1)));
            right.into_iter().chain(up)
        })
    })
}

fn symmetric(config: MazeConfig, position: Coordinates) -> Coordinates {
    Coordinates::new(
        config.width - 1 - position.x,
        config.height - 1 - position.y,
    )
}

fn folded_spatial_bucket(config: MazeConfig, edge: Edge) -> usize {
    debug_assert_eq!(edge, generate_orbit_key(config, edge));
    let x_sum = usize::from(edge.first.x) + usize::from(edge.second.x);
    let y_sum = usize::from(edge.first.y) + usize::from(edge.second.y);
    let x_bucket = (x_sum * 4 / (usize::from(config.width) * 2)).min(3);
    let y_bucket = (y_sum * 4 / (usize::from(config.height) * 2)).min(3);
    y_bucket * 4 + x_bucket
}

fn generate_orbit_key(config: MazeConfig, edge: Edge) -> Edge {
    if !config.symmetry {
        return edge;
    }
    let mirror = Edge::new(
        symmetric(config, edge.first),
        symmetric(config, edge.second),
    );
    edge.min(mirror)
}

fn degree_counts(sample: &GeneratedSample) -> (usize, usize) {
    let mut dead_ends = 0;
    let mut junctions = 0;
    for x in 0..sample.config.width {
        for y in 0..sample.config.height {
            let position = Coordinates::new(x, y);
            let degree = Direction::CARDINALS
                .into_iter()
                .map(|direction| direction.apply_to(position))
                .filter(|&neighbor| {
                    neighbor != position
                        && neighbor.x < sample.config.width
                        && neighbor.y < sample.config.height
                        && sample
                            .final_passages
                            .contains(&Edge::new(position, neighbor))
                })
                .count();
            dead_ends += usize::from(degree == 1);
            junctions += usize::from(degree >= 3);
        }
    }
    (dead_ends, junctions)
}

fn corner_distance(sample: &GeneratedSample) -> usize {
    let width = sample.config.width;
    let height = sample.config.height;
    let start = Coordinates::new(0, 0);
    let target = Coordinates::new(width - 1, height - 1);
    let mut distances = vec![usize::MAX; usize::from(width) * usize::from(height)];
    let mut queue = VecDeque::from([start]);
    distances[start.to_index(width)] = 0;

    while let Some(position) = queue.pop_front() {
        if position == target {
            return distances[position.to_index(width)];
        }
        let distance = distances[position.to_index(width)] + 1;
        for direction in Direction::CARDINALS {
            let neighbor = direction.apply_to(position);
            if neighbor == position
                || neighbor.x >= width
                || neighbor.y >= height
                || !sample
                    .final_passages
                    .contains(&Edge::new(position, neighbor))
            {
                continue;
            }
            let index = neighbor.to_index(width);
            if distances[index] == usize::MAX {
                distances[index] = distance;
                queue.push_back(neighbor);
            }
        }
    }

    panic!("connected diagnostic maze has no corner-to-corner path")
}

fn fnv1a(previous: u64, bytes: &[u8]) -> u64 {
    let mut hash = if previous == 0 {
        0xcbf2_9ce4_8422_2325
    } else {
        previous
    };
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    numerator as f64 / denominator as f64
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn full_game_bytes(game: &GameState) -> Vec<u8> {
    let mut walls: Vec<_> = game
        .wall_entries()
        .into_iter()
        .map(|wall| Edge::new(wall.pos1, wall.pos2))
        .collect();
    walls.sort_unstable();
    walls.dedup();
    let mud: BTreeMap<_, _> = game
        .mud_positions()
        .iter()
        .map(|((first, second), cost)| (Edge::new(first, second), cost))
        .collect();
    let mut cheese = game.cheese_positions();
    cheese.sort_unstable();

    let mut bytes = Vec::new();
    bytes.extend([game.width(), game.height()]);
    bytes.extend(
        u32::try_from(walls.len())
            .expect("diagnostic wall count fits u32")
            .to_le_bytes(),
    );
    for wall in walls {
        bytes.extend([wall.first.x, wall.first.y, wall.second.x, wall.second.y]);
    }
    bytes.extend(
        u32::try_from(mud.len())
            .expect("diagnostic mud count fits u32")
            .to_le_bytes(),
    );
    for (edge, cost) in mud {
        bytes.extend([
            edge.first.x,
            edge.first.y,
            edge.second.x,
            edge.second.y,
            cost,
        ]);
    }
    let player1 = game.player1_position();
    let player2 = game.player2_position();
    bytes.extend([player1.x, player1.y, player2.x, player2.y]);
    bytes.extend(
        u32::try_from(cheese.len())
            .expect("diagnostic cheese count fits u32")
            .to_le_bytes(),
    );
    for position in cheese {
        bytes.extend([position.x, position.y]);
    }
    bytes.extend(game.state_hash().to_le_bytes());
    bytes
}

fn topology_probe_games() -> Vec<(&'static str, GameState)> {
    let symmetric = GameBuilder::new(11, 9)
        .with_random_maze(MazeParams::classic())
        .with_random_positions()
        .with_random_cheese(12, true)
        .build()
        .create(Some(0xA11C_E5E5))
        .expect("symmetric probe game must build");
    let asymmetric = GameBuilder::new(10, 8)
        .with_random_maze(MazeParams {
            symmetric: false,
            ..MazeParams::classic()
        })
        .with_random_positions()
        .with_random_cheese(10, false)
        .build()
        .create(Some(0xA5A5_5A5A))
        .expect("asymmetric probe game must build");
    let singleton_orbit = GameBuilder::new(4, 3)
        .with_random_maze(MazeParams {
            wall_density: 1.0,
            mud_density: 1.0,
            mud_range: 4,
            ..MazeParams::classic()
        })
        .with_random_positions()
        .with_random_cheese(4, true)
        .build()
        .create(Some(0x51A6_1E70))
        .expect("singleton-orbit probe game must build");

    vec![
        ("symmetric", symmetric),
        ("asymmetric", asymmetric),
        ("singleton", singleton_orbit),
    ]
}

fn summarize_classic(width: u8, height: u8, seed_count: u64) -> CorpusSummary {
    let mut summary = CorpusSummary::default();
    for seed in 0..seed_count {
        summary.add(&generate_sample(classic_config(width, height, seed)));
    }
    summary
}

fn report_corpus() -> Vec<String> {
    [("medium", 21, 15, 256), ("huge", 41, 31, 64)]
        .into_iter()
        .map(|(name, width, height, seed_count)| {
            summarize_classic(width, height, seed_count).report(name)
        })
        .collect()
}

fn run_probe_process(profile: &str) -> Vec<String> {
    let output = Command::new(std::env::current_exe().expect("test binary path must be available"))
        .args([
            "--exact",
            "game::maze_generation::distribution_tests::pr9_probe_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("PYRAT_PR9_PROFILE", profile)
        .output()
        .expect("maze probe child process must launch");
    assert!(
        output.status.success(),
        "maze probe child failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8(output.stdout)
        .expect("probe output must be UTF-8")
        .lines()
        .filter_map(|line| {
            line.find(REPORT_PREFIX)
                .or_else(|| line.find(TOPOLOGY_PREFIX))
                .map(|start| line[start..].to_owned())
        })
        .collect()
}

#[test]
#[ignore = "manual multi-process distribution evidence"]
fn pr9_distribution_report() {
    for process in 0..REPORT_PROCESSES {
        let rows = run_probe_process("report");
        assert_eq!(rows.len(), 2, "each report process emits medium and huge");
        for row in rows {
            println!("{row}\tprocess={process}");
        }
    }
}

#[test]
fn connected_generation_is_cross_process_deterministic() {
    let first = run_probe_process("topology");
    assert_eq!(first.len(), 3, "the child emits all topology scenarios");
    for _ in 0..2 {
        assert_eq!(run_probe_process("topology"), first);
    }
}

#[test]
fn connected_generation_is_same_process_deterministic() {
    let expected: Vec<_> = topology_probe_games()
        .into_iter()
        .map(|(name, game)| (name, full_game_bytes(&game)))
        .collect();
    for _ in 0..8 {
        let actual: Vec<_> = topology_probe_games()
            .into_iter()
            .map(|(name, game)| (name, full_game_bytes(&game)))
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn repair_distribution_guardrails() {
    let summary = summarize_classic(21, 15, 128);
    assert!(summary.topology_bytes.len() >= 126);

    let seed_count = summary.seeds as f64;
    let passage_mean = summary.passages as f64 / seed_count;
    let cycle_mean = summary.cycle_rank as f64 / seed_count;
    let repair_edge_mean = summary.repair_edges as f64 / seed_count;
    let repair_orbit_mean = summary.repair_orbits as f64 / seed_count;
    let dead_end_mean = summary.dead_ends as f64 / seed_count;
    let junction_mean = summary.junctions as f64 / seed_count;
    let corner_distance_mean = summary.corner_distance as f64 / seed_count;
    assert!((315.0..=322.0).contains(&passage_mean));
    assert!((1.0..=7.0).contains(&cycle_mean));
    assert!((125.0..=145.0).contains(&repair_edge_mean));
    assert!((60.0..=75.0).contains(&repair_orbit_mean));
    assert!((80.0..=105.0).contains(&dead_end_mean));
    assert!((75.0..=95.0).contains(&junction_mean));
    assert!((40.0..=65.0).contains(&corner_distance_mean));

    let horizontal_rate = ratio(summary.horizontal_repairs, summary.horizontal_candidates);
    let vertical_rate = ratio(summary.vertical_repairs, summary.vertical_candidates);
    assert!((horizontal_rate - vertical_rate).abs() < 0.03);

    let spatial_rates: Vec<_> = summary
        .spatial_repairs
        .iter()
        .zip(summary.spatial_candidates.iter())
        .filter(|(_, candidates)| **candidates > 0)
        .map(|(&repairs, &candidates)| ratio(repairs, candidates))
        .collect();
    let minimum = spatial_rates.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = spatial_rates
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(maximum - minimum < 0.14);

    let mud_rate = ratio(summary.muddy_repair_orbits, summary.repair_orbits);
    assert!((0.08..=0.12).contains(&mud_rate));
}

#[test]
fn repair_mud_uses_one_configured_probability_trial_per_orbit() {
    let mut summary = CorpusSummary::default();
    for seed in 0..256 {
        summary.add(&generate_sample(MazeConfig {
            target_density: 1.0,
            ..classic_config(21, 15, seed)
        }));
    }
    let mud_rate = ratio(summary.muddy_repair_orbits, summary.repair_orbits);
    assert!(
        (0.085..=0.115).contains(&mud_rate),
        "repair-orbit mud rate {mud_rate:.6} should follow p=0.1, not the old p=0.19"
    );
}

#[test]
fn symmetric_border_repair_uses_one_configured_probability_trial_per_orbit() {
    let mut repair_orbits = 0_u64;
    let mut muddy_repair_orbits = 0_u64;
    for seed in 0..256 {
        let sample = generate_sample(MazeConfig {
            connected: false,
            target_density: 1.0,
            ..classic_config(21, 15, seed)
        });
        let repaired: BTreeSet<_> = sample
            .repair_edges()
            .map(|edge| sample.repair_orbit_key(edge))
            .collect();
        let muddy: BTreeSet<_> = sample
            .repair_edges()
            .filter(|edge| sample.mud.contains_key(edge))
            .map(|edge| sample.repair_orbit_key(edge))
            .collect();
        repair_orbits += repaired.len() as u64;
        muddy_repair_orbits += muddy.len() as u64;
    }

    let mud_rate = ratio(muddy_repair_orbits, repair_orbits);
    assert!(
        (0.085..=0.115).contains(&mud_rate),
        "border-repair orbit mud rate {mud_rate:.6} should follow p=0.1, not the old p=0.19"
    );
}

#[test]
fn symmetric_even_even_all_walls_has_one_redundant_mirrored_edge() {
    for seed in 0..32 {
        let sample = generate_sample(MazeConfig {
            width: 4,
            height: 4,
            target_density: 1.0,
            connected: true,
            symmetry: true,
            mud_density: 0.0,
            mud_range: 2,
            seed: Some(seed),
        });
        assert_eq!(sample.final_passages.len(), 16, "seed={seed}");
        assert_eq!(sample.final_passages.len() + 1 - 16, 1, "seed={seed}");
        assert_eq!(sample.repair_edges().count(), 16, "seed={seed}");
    }
}

#[test]
#[ignore = "child process for PR 9 diagnostics"]
fn pr9_probe_child() {
    match std::env::var("PYRAT_PR9_PROFILE").as_deref() {
        Ok("report") => {
            for row in report_corpus() {
                println!("{row}");
            }
        },
        Ok("topology") => {
            for (name, game) in topology_probe_games() {
                println!(
                    "{TOPOLOGY_PREFIX}\t{name}\t{}",
                    hex_bytes(&full_game_bytes(&game))
                );
            }
        },
        profile => panic!("unknown PR 9 diagnostic profile: {profile:?}"),
    }
}
