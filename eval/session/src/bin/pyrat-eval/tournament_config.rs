//! TOML schema for `pyrat-eval tournament run --config <path>`.
//!
//! The struct is serde-symmetric: `Deserialize` reads a `--config` file,
//! `Serialize` writes one back for `--save-as`. Every optional-on-input
//! field is `Option<T>`; defaults live in `tournament_resolve`, not here.
//! Validation runs as part of `resolve()` — the schema only describes the
//! shape of the file.
//!
//! Paths inside this struct are interpreted relative to the config file's
//! parent directory at resolve time. CLI flags carry their own paths
//! resolved from CWD; see `tournament_resolve` for the rules.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Top-level shape of a `tournament_run` config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TournamentConfig {
    pub store_path: Option<PathBuf>,
    pub replay_dir: Option<PathBuf>,
    pub seed: Option<u64>,
    pub format: Option<String>,
    pub target_games_per_matchup: Option<u32>,
    pub max_failures_per_pair: Option<u32>,
    pub max_parallel: Option<u32>,

    #[serde(default)]
    pub game: Option<GameSection>,
    #[serde(default)]
    pub timing: Option<TimingSection>,
    #[serde(default)]
    pub elo: Option<EloSection>,
    #[serde(default)]
    pub players: Vec<PlayerEntry>,
    #[serde(default)]
    pub gauntlet: Option<GauntletSection>,
}

/// Either a named preset (e.g. `"tiny"`) or a custom triplet (width,
/// height, cheese) with optional `symmetric` flag. Validation that exactly
/// one of those two forms is set happens in `resolve()`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GameSection {
    pub preset: Option<String>,
    pub width: Option<u8>,
    pub height: Option<u8>,
    pub cheese: Option<u16>,
    pub symmetric: Option<bool>,
    pub max_turns: Option<u16>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TimingSection {
    pub move_timeout_ms: Option<u32>,
    pub preprocessing_timeout_ms: Option<u32>,
    pub startup_timeout_ms: Option<u32>,
    pub configure_timeout_ms: Option<u32>,
    pub network_grace_ms: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EloSection {
    pub anchor: Option<String>,
    pub anchor_elo: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlayerEntry {
    pub id: String,
    pub command: String,
    pub working_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GauntletSection {
    pub challenger: String,
    pub opponents: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(cfg: &TournamentConfig) -> TournamentConfig {
        let serialized = toml::to_string(cfg).expect("serialize");
        toml::from_str(&serialized).expect("deserialize")
    }

    #[test]
    fn preset_game_round_trips() {
        let cfg = TournamentConfig {
            format: Some("round_robin".into()),
            target_games_per_matchup: Some(5),
            game: Some(GameSection {
                preset: Some("tiny".into()),
                max_turns: Some(30),
                ..Default::default()
            }),
            players: vec![
                PlayerEntry {
                    id: "greedy".into(),
                    command: "cargo run --release".into(),
                    working_dir: Some("../botpack/greedy".into()),
                },
                PlayerEntry {
                    id: "smart_random".into(),
                    command: "cargo run --release".into(),
                    working_dir: Some("../botpack/smart-random".into()),
                },
            ],
            ..Default::default()
        };
        assert_eq!(round_trip(&cfg), cfg);
    }

    #[test]
    fn custom_game_round_trips() {
        let cfg = TournamentConfig {
            format: Some("round_robin".into()),
            target_games_per_matchup: Some(3),
            game: Some(GameSection {
                width: Some(7),
                height: Some(7),
                cheese: Some(5),
                symmetric: Some(true),
                max_turns: Some(50),
                ..Default::default()
            }),
            players: vec![PlayerEntry {
                id: "a".into(),
                command: "echo".into(),
                working_dir: None,
            }],
            ..Default::default()
        };
        assert_eq!(round_trip(&cfg), cfg);
    }

    #[test]
    fn gauntlet_section_round_trips() {
        let cfg = TournamentConfig {
            format: Some("gauntlet".into()),
            game: Some(GameSection {
                preset: Some("tiny".into()),
                ..Default::default()
            }),
            players: vec![
                PlayerEntry {
                    id: "greedy".into(),
                    command: "cargo run --release".into(),
                    working_dir: None,
                },
                PlayerEntry {
                    id: "smart_random".into(),
                    command: "cargo run --release".into(),
                    working_dir: None,
                },
            ],
            gauntlet: Some(GauntletSection {
                challenger: "greedy".into(),
                opponents: vec!["smart_random".into()],
            }),
            ..Default::default()
        };
        assert_eq!(round_trip(&cfg), cfg);
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        let raw = "format = \"round_robin\"\nbogus = true\n";
        let err = toml::from_str::<TournamentConfig>(raw).unwrap_err();
        assert!(
            err.to_string().contains("bogus") || err.to_string().contains("unknown"),
            "expected unknown-field error, got: {err}"
        );
    }
}
