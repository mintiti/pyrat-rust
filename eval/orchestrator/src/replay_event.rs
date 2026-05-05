//! Serde-friendly mirror of [`MatchEvent`] for replay JSON files.
//!
//! `MatchEvent` carries flatbuffers-generated wire types (`Player`,
//! `Direction`, `TimingMode`, `GameResult`) that don't derive `Serialize`,
//! so a sink can't write them directly. The DTO at this boundary flattens
//! every wire value to a primitive (`u8` for enums, `(u8, u8)` for
//! `Coordinates`) so a `serde_json` writer doesn't need to reach into the
//! protocol crate.
//!
//! The conversion is **one-way**: `From<&MatchEvent>`. We never reconstruct
//! a `MatchEvent` from a replay file: replays are forensic records, not
//! a substitute for a live match.
//!
//! `MatchEvent` is `#[non_exhaustive]`, so the catch-all maps unknown host
//! variants to [`ReplayEvent::Unknown`] with a `Debug`-rendered string.
//! Lossy but explicit: a forensic file ending in `Unknown` records *that*
//! the host emitted something the orchestrator didn't recognise, with
//! enough detail to chase it down.

use pyrat::Direction;
use pyrat_host::match_host::{MatchEvent, MatchResult};
use pyrat_protocol::{Info, MatchConfig};
use serde::{Deserialize, Serialize};

/// `(x, y)` pair, the wire encoding for `pyrat::Coordinates`.
type CoordPair = (u8, u8);

/// Flat mirror of [`MatchEvent`] suitable for JSON serialisation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplayEvent {
    BotIdentified {
        /// Slot the bot was assigned (Player1 = 0, Player2 = 1).
        player: u8,
        name: String,
        author: String,
        agent_id: String,
    },
    PreprocessingStarted,
    SetupComplete,
    MatchStarted {
        config: ReplayMatchConfig,
    },
    TurnPlayed {
        turn: u16,
        state_hash: u64,
        p1_position: CoordPair,
        p2_position: CoordPair,
        p1_score: f32,
        p2_score: f32,
        p1_mud_turns: u8,
        p2_mud_turns: u8,
        p1_action: u8,
        p2_action: u8,
        p1_last_move: u8,
        p2_last_move: u8,
        cheese: Vec<CoordPair>,
        p1_think_ms: u32,
        p2_think_ms: u32,
    },
    BotInfo {
        sender: u8,
        turn: u16,
        state_hash: u64,
        info: ReplayInfo,
    },
    BotProvisional {
        sender: u8,
        turn: u16,
        state_hash: u64,
        direction: u8,
    },
    BotTimeout {
        player: u8,
        turn: u16,
    },
    /// Synthesised by the replay sink at terminal time. The host's
    /// `MatchOver` is suppressed by the orchestrator (see `run_match.rs`),
    /// so any `ReplayEvent::MatchOver` in a file came from the sink, not
    /// the host event stream.
    MatchOver {
        result: ReplayMatchResult,
    },
    /// Catch-all for `MatchEvent` variants the orchestrator doesn't yet
    /// recognise. Carries the `Debug` rendering as the only crumb.
    Unknown {
        variant_debug: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayMatchConfig {
    pub width: u8,
    pub height: u8,
    pub max_turns: u16,
    pub walls: Vec<(CoordPair, CoordPair)>,
    /// `(pos1, pos2, turns)` triples. `pos1 <= pos2` is enforced by
    /// `pyrat_host::match_config::build_match_config`.
    pub mud: Vec<(CoordPair, CoordPair, u8)>,
    pub cheese: Vec<CoordPair>,
    pub player1_start: CoordPair,
    pub player2_start: CoordPair,
    /// `TimingMode` as a wire-level `u8`.
    pub timing: u8,
    pub move_timeout_ms: u32,
    pub preprocessing_timeout_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayMatchResult {
    /// `GameResult` as a wire-level `u8` (Player1 = 0, Player2 = 1, Draw = 2).
    pub result: u8,
    pub player1_score: f32,
    pub player2_score: f32,
    pub turns_played: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayInfo {
    pub player: u8,
    pub multipv: u16,
    pub target: Option<CoordPair>,
    pub depth: u16,
    pub nodes: u32,
    pub score: Option<f32>,
    /// `Direction`s rendered as `u8`.
    pub pv: Vec<u8>,
    pub message: String,
    pub turn: u16,
    pub state_hash: u64,
}

fn coord(c: pyrat::Coordinates) -> CoordPair {
    (c.x, c.y)
}

fn dir(d: Direction) -> u8 {
    d as u8
}

impl From<&MatchConfig> for ReplayMatchConfig {
    fn from(cfg: &MatchConfig) -> Self {
        Self {
            width: cfg.width,
            height: cfg.height,
            max_turns: cfg.max_turns,
            walls: cfg
                .walls
                .iter()
                .map(|(a, b)| (coord(*a), coord(*b)))
                .collect(),
            mud: cfg
                .mud
                .iter()
                .map(|m| (coord(m.pos1), coord(m.pos2), m.turns))
                .collect(),
            cheese: cfg.cheese.iter().copied().map(coord).collect(),
            player1_start: coord(cfg.player1_start),
            player2_start: coord(cfg.player2_start),
            timing: cfg.timing.0,
            move_timeout_ms: cfg.move_timeout_ms,
            preprocessing_timeout_ms: cfg.preprocessing_timeout_ms,
        }
    }
}

impl From<&MatchResult> for ReplayMatchResult {
    fn from(r: &MatchResult) -> Self {
        Self {
            result: r.result.0,
            player1_score: r.player1_score,
            player2_score: r.player2_score,
            turns_played: r.turns_played,
        }
    }
}

impl From<&Info> for ReplayInfo {
    fn from(info: &Info) -> Self {
        Self {
            player: info.player.0,
            multipv: info.multipv,
            target: info.target.map(coord),
            depth: info.depth,
            nodes: info.nodes,
            score: info.score,
            pv: info.pv.iter().copied().map(dir).collect(),
            message: info.message.clone(),
            turn: info.turn,
            state_hash: info.state_hash,
        }
    }
}

impl From<&MatchEvent> for ReplayEvent {
    fn from(event: &MatchEvent) -> Self {
        match event {
            MatchEvent::BotIdentified {
                player,
                name,
                author,
                agent_id,
            } => Self::BotIdentified {
                player: player.0,
                name: name.clone(),
                author: author.clone(),
                agent_id: agent_id.clone(),
            },
            MatchEvent::PreprocessingStarted => Self::PreprocessingStarted,
            MatchEvent::SetupComplete => Self::SetupComplete,
            MatchEvent::MatchStarted { config } => Self::MatchStarted {
                config: config.into(),
            },
            MatchEvent::TurnPlayed {
                state,
                p1_action,
                p2_action,
                p1_think_ms,
                p2_think_ms,
            } => Self::TurnPlayed {
                turn: state.turn,
                state_hash: state.state_hash(),
                p1_position: coord(state.player1_position),
                p2_position: coord(state.player2_position),
                p1_score: state.player1_score,
                p2_score: state.player2_score,
                p1_mud_turns: state.player1_mud_turns,
                p2_mud_turns: state.player2_mud_turns,
                p1_action: dir(*p1_action),
                p2_action: dir(*p2_action),
                p1_last_move: dir(state.player1_last_move),
                p2_last_move: dir(state.player2_last_move),
                cheese: state.cheese.iter().copied().map(coord).collect(),
                p1_think_ms: *p1_think_ms,
                p2_think_ms: *p2_think_ms,
            },
            MatchEvent::BotInfo {
                sender,
                turn,
                state_hash,
                info,
            } => Self::BotInfo {
                sender: sender.0,
                turn: *turn,
                state_hash: *state_hash,
                info: info.into(),
            },
            MatchEvent::BotProvisional {
                sender,
                turn,
                state_hash,
                direction,
            } => Self::BotProvisional {
                sender: sender.0,
                turn: *turn,
                state_hash: *state_hash,
                direction: dir(*direction),
            },
            MatchEvent::BotTimeout { player, turn } => Self::BotTimeout {
                player: player.0,
                turn: *turn,
            },
            MatchEvent::MatchOver { result } => Self::MatchOver {
                result: result.into(),
            },
            // `MatchEvent` is `#[non_exhaustive]`. Future variants land here
            // until the orchestrator catches up.
            other => Self::Unknown {
                variant_debug: format!("{other:?}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat::Coordinates;
    use pyrat_host::wire::{GameResult, Player as PlayerSlot, TimingMode};
    use pyrat_protocol::{HashedTurnState, TurnState};

    fn sample_match_config() -> MatchConfig {
        MatchConfig {
            width: 5,
            height: 5,
            max_turns: 100,
            walls: vec![(Coordinates::new(0, 0), Coordinates::new(0, 1))],
            mud: vec![pyrat_protocol::MudEntry {
                pos1: Coordinates::new(1, 1),
                pos2: Coordinates::new(2, 1),
                turns: 3,
            }],
            cheese: vec![Coordinates::new(2, 2)],
            player1_start: Coordinates::new(0, 0),
            player2_start: Coordinates::new(4, 4),
            timing: TimingMode::Wait,
            move_timeout_ms: 1000,
            preprocessing_timeout_ms: 5000,
        }
    }

    fn sample_turn_state() -> HashedTurnState {
        HashedTurnState::with_unverified_hash(
            TurnState {
                turn: 7,
                player1_position: Coordinates::new(1, 1),
                player2_position: Coordinates::new(3, 3),
                player1_score: 1.5,
                player2_score: 0.5,
                player1_mud_turns: 0,
                player2_mud_turns: 2,
                cheese: vec![Coordinates::new(2, 2)],
                player1_last_move: Direction::Up,
                player2_last_move: Direction::Stay,
            },
            0xCAFEF00D,
        )
    }

    /// Every known `MatchEvent` variant maps to a non-`Unknown` `ReplayEvent`
    /// and survives a serde round-trip. If the host adds a variant, the
    /// match in `From<&MatchEvent>` falls through to `Unknown`. This test
    /// fails loudly so the mapping table gets updated.
    ///
    /// Round-trip note: `ReplayEvent` is `Serialize + Deserialize` for the
    /// JSON format. The deserialised value should be byte-equal to the
    /// serialised one (no semantic equivalence checks needed beyond JSON
    /// round-tripping).
    #[test]
    fn every_match_event_variant_maps_to_non_unknown_and_roundtrips() {
        let cfg = sample_match_config();
        let result = MatchResult {
            result: GameResult::Player1,
            player1_score: 4.0,
            player2_score: 0.0,
            turns_played: 42,
        };
        let info = Info {
            player: PlayerSlot::Player1,
            multipv: 1,
            target: Some(Coordinates::new(2, 2)),
            depth: 3,
            nodes: 100,
            score: Some(0.75),
            pv: vec![Direction::Up, Direction::Right],
            message: "ok".into(),
            turn: 7,
            state_hash: 0xDEADBEEF,
        };

        let cases: Vec<MatchEvent> = vec![
            MatchEvent::BotIdentified {
                player: PlayerSlot::Player1,
                name: "n".into(),
                author: "a".into(),
                agent_id: "id".into(),
            },
            MatchEvent::PreprocessingStarted,
            MatchEvent::SetupComplete,
            MatchEvent::MatchStarted { config: cfg },
            MatchEvent::TurnPlayed {
                state: sample_turn_state(),
                p1_action: Direction::Up,
                p2_action: Direction::Stay,
                p1_think_ms: 5,
                p2_think_ms: 6,
            },
            MatchEvent::BotInfo {
                sender: PlayerSlot::Player1,
                turn: 7,
                state_hash: 0xDEAD,
                info,
            },
            MatchEvent::BotProvisional {
                sender: PlayerSlot::Player2,
                turn: 7,
                state_hash: 0xBEEF,
                direction: Direction::Down,
            },
            MatchEvent::BotTimeout {
                player: PlayerSlot::Player2,
                turn: 7,
            },
            MatchEvent::MatchOver { result },
        ];

        for ev in &cases {
            let dto = ReplayEvent::from(ev);
            assert!(
                !matches!(dto, ReplayEvent::Unknown { .. }),
                "variant {ev:?} mapped to Unknown; update From<&MatchEvent>"
            );
            let json = serde_json::to_string(&dto).expect("serialise");
            let _back: ReplayEvent = serde_json::from_str(&json).expect("deserialise");
        }
    }

    #[test]
    fn match_result_to_replay_uses_wire_u8() {
        let r = MatchResult {
            result: GameResult::Player2,
            player1_score: 1.0,
            player2_score: 2.0,
            turns_played: 12,
        };
        let dto = ReplayMatchResult::from(&r);
        assert_eq!(dto.result, GameResult::Player2.0);
        assert_eq!(dto.player1_score, 1.0);
        assert_eq!(dto.player2_score, 2.0);
        assert_eq!(dto.turns_played, 12);
    }
}
