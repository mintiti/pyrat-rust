//! `--save-as` materializer. Inverse of `tournament_resolve::resolve()`:
//! projects a `ResolvedRun` back into the `TournamentConfig` TOML schema
//! so a flag-driven run can be committed as a reusable spec.
//!
//! The projection draws the blueprint/instance line: explicit seeds
//! persist, generated and store-derived seeds are omitted (a saved
//! blueprint stays decoupled from any one instance's seed), and
//! per-invocation options (`--save-as`, `--results-json`, `--resume`)
//! never appear in the output. Paths are rebased relative to the save
//! directory so the spec works on other machines.

use std::fs;
use std::path::{Path, PathBuf};

use pyrat_eval::ResolvedPlayer;

use crate::game_config_build::{GameShape, ResolvedGame};
use crate::tournament_config::{
    EloSection, GameSection, GauntletSection, PlayerEntry, TimingSection, TournamentConfig,
};
use crate::tournament_resolve::{FormatChoice, LaunchMode, NewSeed, ResolvedRun};

pub(crate) fn write_save_as(
    resolved: &ResolvedRun,
    save_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let save_dir = save_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(save_dir).map_err(|e| {
        format!(
            "--save-as: failed to create parent directory {}: {e}",
            save_dir.display()
        )
    })?;
    let cfg = to_saveable_config(resolved, save_dir)?;
    let toml_text = toml::to_string_pretty(&cfg)?;
    fs::write(save_path, toml_text)
        .map_err(|e| format!("--save-as: failed to write {}: {e}", save_path.display()))?;
    Ok(())
}

/// Project a `ResolvedRun` back into the `TournamentConfig` shape that
/// `--config` deserializes. Paths get rebased to be relative to
/// `save_dir` if possible, otherwise written absolute. Implicit and
/// store-on-resume seeds are intentionally omitted (a saved blueprint
/// stays decoupled from any one instance's seed).
///
/// Errs when a player can't be expressed in the TOML schema (embedded
/// bots) — refusing beats writing a spec that only fails on reload.
fn to_saveable_config(resolved: &ResolvedRun, save_dir: &Path) -> Result<TournamentConfig, String> {
    let format = match &resolved.format {
        FormatChoice::RoundRobin => "round_robin",
        FormatChoice::Gauntlet { .. } => "gauntlet",
    };
    let game = Some(game_section_from(&resolved.game));
    let timing = Some(TimingSection {
        move_timeout_ms: Some(resolved.timing.move_timeout_ms),
        preprocessing_timeout_ms: Some(resolved.timing.preprocessing_timeout_ms),
        startup_timeout_ms: Some(resolved.timing.startup_timeout_ms),
        configure_timeout_ms: Some(resolved.timing.configure_timeout_ms),
        network_grace_ms: Some(resolved.timing.network_grace_ms),
    });
    let elo = Some(EloSection {
        anchor: Some(resolved.anchor.clone()),
        anchor_elo: Some(resolved.anchor_elo),
    });
    let players = resolved
        .players
        .iter()
        .map(|p| player_entry_from(p, save_dir))
        .collect::<Result<Vec<_>, _>>()?;
    let gauntlet = match &resolved.format {
        FormatChoice::Gauntlet {
            challenger,
            opponents,
        } => Some(GauntletSection {
            challenger: challenger.clone(),
            opponents: opponents.clone(),
        }),
        FormatChoice::RoundRobin => None,
    };
    // Total match documents the policy: a blueprint never inherits an
    // instance's seed. (Resume + --save-as is unreachable via clap's
    // conflicts_with, but the projection stays total and honest.)
    let seed = match resolved.mode {
        LaunchMode::New {
            seed: NewSeed::Explicit(s),
        } => Some(s),
        LaunchMode::New {
            seed: NewSeed::Generated(_),
        }
        | LaunchMode::Resume { .. } => None,
    };

    Ok(TournamentConfig {
        store_path: Some(make_relative_or_absolute(&resolved.store_path, save_dir)),
        replay_dir: resolved
            .replay_dir
            .as_ref()
            .map(|p| make_relative_or_absolute(p, save_dir)),
        seed,
        format: Some(format.into()),
        target_games_per_matchup: Some(resolved.target_games_per_matchup),
        max_failures_per_pair: Some(resolved.max_failures_per_pair),
        max_parallel: Some(resolved.max_parallel),
        game,
        timing,
        elo,
        players,
        gauntlet,
    })
}

fn game_section_from(game: &ResolvedGame) -> GameSection {
    let max_turns = game.max_turns.map(|n| n.get());
    match &game.shape {
        GameShape::Preset { name } => GameSection {
            preset: Some(name.clone()),
            max_turns,
            ..Default::default()
        },
        GameShape::Custom {
            width,
            height,
            cheese,
            symmetric,
        } => GameSection {
            width: Some(*width),
            height: Some(*height),
            cheese: Some(*cheese),
            symmetric: Some(*symmetric),
            max_turns,
            ..Default::default()
        },
    }
}

fn player_entry_from(player: &ResolvedPlayer, save_dir: &Path) -> Result<PlayerEntry, String> {
    use pyrat_orchestrator::PlayerSpec;
    match &player.spec {
        PlayerSpec::Subprocess {
            command,
            working_dir,
            ..
        } => Ok(PlayerEntry {
            id: player.id.clone(),
            command: command.clone(),
            working_dir: working_dir
                .as_ref()
                .map(|p| make_relative_or_absolute(p, save_dir)),
        }),
        // Embedded bots can't be serialized (factories are closures).
        // Unreachable from today's resolver (it only builds Subprocess),
        // but refusing here beats a future library path silently writing
        // a spec that only fails on reload.
        _ => Err(format!(
            "--save-as: player `{}` is not a subprocess bot and cannot be written to TOML",
            player.id
        )),
    }
}

/// Best-effort absolute form: if the path's parent exists, canonicalize
/// it and rejoin the filename. If not, fall back to the path as-is.
/// Used as the base step in path rebasing so non-existent targets
/// (`store_path`, `replay_dir`) don't blow up `canonicalize`.
fn absolutize_path(path: &Path) -> PathBuf {
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        if let Ok(canon_parent) = fs::canonicalize(parent) {
            return canon_parent.join(name);
        }
    }
    path.to_path_buf()
}

/// Rebase `path` relative to `save_dir`, walking up with `..` when the
/// two live in sibling or unrelated subtrees. Without this, a
/// `--save-as configs/ladder.toml` from the repo root would serialize
/// `working_dir = "/absolute/path/to/botpack/greedy"` because `botpack/`
/// and `configs/` don't share a strip_prefix-compatible prefix — and
/// the saved spec would then only work on the author's machine.
///
/// Equivalent to `pathdiff::diff_paths`; ~15 lines of std, no new dep.
fn make_relative_or_absolute(path: &Path, save_dir: &Path) -> PathBuf {
    let abs_path = absolutize_path(path);
    let abs_save_dir = fs::canonicalize(save_dir).unwrap_or_else(|_| save_dir.to_path_buf());
    make_relative_to(&abs_path, &abs_save_dir)
}

/// Compute a relative path from `base` to `target`. Both paths must be
/// absolute and already canonicalized (or absolutized) — callers handle
/// that step so this function is purely structural.
fn make_relative_to(target: &Path, base: &Path) -> PathBuf {
    use std::path::Component;
    let target_components: Vec<Component<'_>> = target.components().collect();
    let base_components: Vec<Component<'_>> = base.components().collect();
    let common = target_components
        .iter()
        .zip(base_components.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut result = PathBuf::new();
    for _ in &base_components[common..] {
        result.push("..");
    }
    for c in &target_components[common..] {
        result.push(c.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tournament_resolve::ResolvedTiming;
    use pyrat_orchestrator::PlayerSpec;
    use std::num::NonZeroU16;

    fn fixture_resolved(save_as: Option<PathBuf>) -> ResolvedRun {
        ResolvedRun {
            players: vec![ResolvedPlayer {
                id: "greedy".into(),
                spec: PlayerSpec::Subprocess {
                    agent_id: "greedy".into(),
                    command: "cargo run --release".into(),
                    working_dir: Some(PathBuf::from("/tmp/work/botpack/greedy")),
                },
            }],
            game: ResolvedGame {
                shape: GameShape::Preset {
                    name: "tiny".into(),
                },
                max_turns: NonZeroU16::new(50),
            },
            timing: ResolvedTiming {
                move_timeout_ms: 1000,
                preprocessing_timeout_ms: 10_000,
                startup_timeout_ms: 30_000,
                configure_timeout_ms: 5000,
                network_grace_ms: 50,
            },
            format: FormatChoice::RoundRobin,
            target_games_per_matchup: 5,
            max_failures_per_pair: 1,
            max_parallel: 2,
            mode: LaunchMode::New {
                seed: NewSeed::Generated(123),
            },
            store_path: PathBuf::from("/tmp/work/ratings.db"),
            replay_dir: None,
            anchor: "greedy".into(),
            anchor_elo: 1000.0,
            results_json: None,
            save_as,
        }
    }

    #[test]
    fn save_as_omits_implicit_seed() {
        let resolved = fixture_resolved(Some(PathBuf::from("/tmp/out.toml")));
        let cfg = to_saveable_config(&resolved, Path::new("/tmp")).expect("project");
        assert!(cfg.seed.is_none(), "Generated seed should not be saved");
    }

    #[test]
    fn save_as_keeps_explicit_seed() {
        let mut resolved = fixture_resolved(Some(PathBuf::from("/tmp/out.toml")));
        resolved.mode = LaunchMode::New {
            seed: NewSeed::Explicit(42),
        };
        let cfg = to_saveable_config(&resolved, Path::new("/tmp")).expect("project");
        assert_eq!(cfg.seed, Some(42));
    }

    #[test]
    fn save_as_omits_seed_on_resume_mode() {
        let mut resolved = fixture_resolved(Some(PathBuf::from("/tmp/out.toml")));
        resolved.mode = LaunchMode::Resume {
            id: pyrat_eval_store::TournamentId(1),
            seed_assert: None,
        };
        let cfg = to_saveable_config(&resolved, Path::new("/tmp")).expect("project");
        assert!(cfg.seed.is_none());
    }

    #[test]
    fn save_as_rebases_paths_within_save_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bot_dir = tmp.path().join("bots/greedy");
        std::fs::create_dir_all(&bot_dir).unwrap();
        let store = tmp.path().join("ratings.db");

        let resolved = ResolvedRun {
            players: vec![ResolvedPlayer {
                id: "greedy".into(),
                spec: PlayerSpec::Subprocess {
                    agent_id: "greedy".into(),
                    command: "cargo run --release".into(),
                    working_dir: Some(bot_dir.clone()),
                },
            }],
            store_path: store.clone(),
            ..fixture_resolved(None)
        };
        let cfg = to_saveable_config(&resolved, tmp.path()).expect("project");

        // Both paths should be relative to save_dir.
        assert_eq!(cfg.store_path.as_deref(), Some(Path::new("ratings.db")));
        assert_eq!(
            cfg.players[0].working_dir.as_deref(),
            Some(Path::new("bots/greedy"))
        );
    }

    #[test]
    fn save_as_rebases_paths_outside_save_dir_with_dotdot() {
        // save_dir and bot_dir live in unrelated tempdirs (sibling-ish
        // under /var/folders or /tmp). The rebased path must walk up
        // with `..` and back down — without this, the saved TOML would
        // contain absolute paths that only work on the original machine.
        let save_dir = tempfile::tempdir().expect("save dir");
        let bots_root = tempfile::tempdir().expect("bots dir");
        let bot_dir = bots_root.path().join("greedy");
        std::fs::create_dir_all(&bot_dir).unwrap();

        let resolved = ResolvedRun {
            players: vec![ResolvedPlayer {
                id: "greedy".into(),
                spec: PlayerSpec::Subprocess {
                    agent_id: "greedy".into(),
                    command: "cargo run --release".into(),
                    working_dir: Some(bot_dir.clone()),
                },
            }],
            store_path: bot_dir.join("ratings.db"),
            ..fixture_resolved(None)
        };
        let cfg = to_saveable_config(&resolved, save_dir.path()).expect("project");

        // Round-trip: the relative working_dir, joined to save_dir,
        // must canonicalize back to bot_dir.
        let written = cfg.players[0]
            .working_dir
            .as_ref()
            .expect("working_dir present");
        let resolved_back =
            std::fs::canonicalize(save_dir.path().join(written)).expect("canonicalize round-trip");
        let bot_dir_canon = std::fs::canonicalize(&bot_dir).expect("canonicalize bot_dir");
        assert_eq!(resolved_back, bot_dir_canon);
        // And the path should contain `..` since it crosses subtrees.
        assert!(
            written
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
            "expected `..` in rebased path, got: {written:?}",
        );
    }

    #[test]
    fn make_relative_to_sibling_directory() {
        let target = Path::new("/tmp/botpack/greedy");
        let base = Path::new("/tmp/configs");
        assert_eq!(
            make_relative_to(target, base),
            PathBuf::from("../botpack/greedy")
        );
    }

    #[test]
    fn make_relative_to_nested_directory() {
        let target = Path::new("/tmp/configs/foo");
        let base = Path::new("/tmp");
        assert_eq!(make_relative_to(target, base), PathBuf::from("configs/foo"));
    }

    #[test]
    fn make_relative_to_parent_directory() {
        let target = Path::new("/tmp/a");
        let base = Path::new("/tmp/a/b");
        assert_eq!(make_relative_to(target, base), PathBuf::from(".."));
    }

    #[test]
    fn make_relative_to_same_directory_is_dot() {
        let target = Path::new("/tmp/a");
        let base = Path::new("/tmp/a");
        assert_eq!(make_relative_to(target, base), PathBuf::from("."));
    }

    #[test]
    fn save_as_handles_nonexistent_store_path() {
        // store_path points at a file inside save_dir that doesn't exist
        // yet (typical first-run scenario). The serializer must not panic
        // on canonicalize-of-missing.
        let save_dir = tempfile::tempdir().expect("save dir");
        let nonexistent_store = save_dir.path().join("not-yet/ratings.db");
        std::fs::create_dir_all(nonexistent_store.parent().unwrap()).unwrap();

        let resolved = ResolvedRun {
            store_path: nonexistent_store.clone(),
            ..fixture_resolved(None)
        };
        let cfg = to_saveable_config(&resolved, save_dir.path()).expect("project");

        // Should produce a relative path.
        assert_eq!(
            cfg.store_path.as_deref(),
            Some(Path::new("not-yet/ratings.db"))
        );
    }

    /// Field-wise projection completeness: every durable field of an
    /// all-non-default `ResolvedRun` survives project → TOML →
    /// re-resolve. A field added to `ResolvedRun` but forgotten in
    /// `to_saveable_config` fails here — the resolver would silently
    /// refill its default, which the success-only roundtrip e2e can't
    /// detect.
    #[test]
    fn projection_round_trips_every_field() {
        use crate::tournament_resolve::{resolve_loaded, LoadedConfig, ResolvedTiming};

        // A root that exists nowhere, so canonicalize never rewrites
        // paths and the comparison stays purely structural.
        let work = Path::new("/nonexistent-pyrat/work");
        let players = vec![
            ResolvedPlayer {
                id: "champ".into(),
                spec: PlayerSpec::Subprocess {
                    agent_id: "champ".into(),
                    command: "./champ --fast".into(),
                    working_dir: Some(work.join("bots/champ")),
                },
            },
            ResolvedPlayer {
                id: "rando".into(),
                spec: PlayerSpec::Subprocess {
                    agent_id: "rando".into(),
                    command: "./rando".into(),
                    working_dir: Some(work.join("bots/rando")),
                },
            },
        ];
        let resolved = ResolvedRun {
            players,
            game: ResolvedGame {
                shape: GameShape::Custom {
                    width: 9,
                    height: 7,
                    cheese: 13,
                    symmetric: false,
                },
                max_turns: NonZeroU16::new(123),
            },
            timing: ResolvedTiming {
                move_timeout_ms: 11,
                preprocessing_timeout_ms: 22,
                startup_timeout_ms: 33,
                configure_timeout_ms: 44,
                network_grace_ms: 55,
            },
            format: FormatChoice::Gauntlet {
                challenger: "champ".into(),
                opponents: vec!["rando".into()],
            },
            target_games_per_matchup: 7,
            max_failures_per_pair: 4,
            max_parallel: 3,
            mode: LaunchMode::New {
                seed: NewSeed::Explicit(99),
            },
            store_path: work.join("scores/ratings.db"),
            replay_dir: Some(work.join("replays")),
            anchor: "rando".into(),
            anchor_elo: 1234.5,
            results_json: None,
            save_as: None,
        };

        let cfg = to_saveable_config(&resolved, work).expect("project");
        let toml_text = toml::to_string_pretty(&cfg).expect("serialize");
        let reparsed = toml::from_str(&toml_text).expect("parse");

        let loaded = LoadedConfig {
            config: reparsed,
            dir: work.to_path_buf(),
            stem: Some("ladder".into()),
        };
        let mut never = || panic!("explicit seed must persist; generator must not run");
        let back = resolve_loaded(crate::empty_run_args(), Some(loaded), &mut never)
            .expect("re-resolve the saved spec");

        assert_eq!(back.game, resolved.game);
        assert_eq!(back.timing, resolved.timing);
        assert_eq!(back.format, resolved.format);
        assert_eq!(back.target_games_per_matchup, 7);
        assert_eq!(back.max_failures_per_pair, 4);
        assert_eq!(back.max_parallel, 3);
        assert_eq!(
            back.mode,
            LaunchMode::New {
                seed: NewSeed::Explicit(99)
            }
        );
        assert_eq!(back.store_path, resolved.store_path);
        assert_eq!(back.replay_dir, resolved.replay_dir);
        assert_eq!(back.anchor, "rando");
        assert_eq!(back.anchor_elo, 1234.5);
        // ResolvedPlayer carries a closure-bearing spec (no PartialEq);
        // compare the durable fields.
        assert_eq!(back.players.len(), resolved.players.len());
        for (b, orig) in back.players.iter().zip(&resolved.players) {
            assert_eq!(b.id, orig.id);
            match (&b.spec, &orig.spec) {
                (
                    PlayerSpec::Subprocess {
                        command: bc,
                        working_dir: bw,
                        ..
                    },
                    PlayerSpec::Subprocess {
                        command: oc,
                        working_dir: ow,
                        ..
                    },
                ) => {
                    assert_eq!(bc, oc);
                    assert_eq!(bw, ow);
                },
                _ => panic!("expected subprocess players"),
            }
        }
    }

    #[test]
    fn write_save_as_round_trips_through_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bot_dir = tmp.path().join("bots/greedy");
        std::fs::create_dir_all(&bot_dir).unwrap();
        let save_path = tmp.path().join("ladder.toml");

        let resolved = ResolvedRun {
            players: vec![ResolvedPlayer {
                id: "greedy".into(),
                spec: PlayerSpec::Subprocess {
                    agent_id: "greedy".into(),
                    command: "cargo run --release".into(),
                    working_dir: Some(bot_dir),
                },
            }],
            store_path: tmp.path().join("ratings.db"),
            save_as: Some(save_path.clone()),
            ..fixture_resolved(None)
        };

        write_save_as(&resolved, &save_path).expect("write");

        let raw = std::fs::read_to_string(&save_path).expect("read back");
        let parsed: TournamentConfig = toml::from_str(&raw).expect("parse");
        assert_eq!(parsed.format.as_deref(), Some("round_robin"));
        assert_eq!(parsed.target_games_per_matchup, Some(5));
        assert_eq!(parsed.players[0].id, "greedy");
        assert!(parsed.seed.is_none(), "Generated seed must not appear");
    }
}
