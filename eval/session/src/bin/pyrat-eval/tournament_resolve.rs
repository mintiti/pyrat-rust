//! Resolver: combines defaults, optional TOML config, and CLI flag
//! overrides into a single `ResolvedRun`. The CLI binary uses
//! `resolve()`; tests use `resolve_loaded()` directly to skip the disk
//! read and inject a deterministic seed source.
//!
//! Precedence per field: explicit flag wins, else config value, else
//! default. Defaults live here, not in clap — see [`crate::RunArgs`].

#![allow(dead_code)] // build_game_config arrives in Chunk 5; consumers wire in.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use pyrat_eval::ResolvedPlayer;
use pyrat_eval_store::TournamentId;
use pyrat_orchestrator::PlayerSpec;

use crate::game_config_build::ResolvedGameChoice;
use crate::tournament_config::{GameSection, PlayerEntry, TournamentConfig};
use crate::{BotArg, RunArgs};

// ── Defaults (live in the resolver, not in clap) ─────────────────────

const DEFAULT_GAMES: u32 = 5;
const DEFAULT_MAX_FAILURES: u32 = 1;
const DEFAULT_MAX_PARALLEL: u32 = 2;
const DEFAULT_MOVE_TIMEOUT_MS: u32 = 1000;
const DEFAULT_PREP_TIMEOUT_MS: u32 = 10_000;
const DEFAULT_STARTUP_TIMEOUT_MS: u32 = 30_000;
const DEFAULT_CONFIGURE_TIMEOUT_MS: u32 = 5_000;
const DEFAULT_NETWORK_GRACE_MS: u32 = 50;
const DEFAULT_ANCHOR_ELO: f64 = 1000.0;
const DEFAULT_STORE_FILENAME: &str = "ratings.db";
const FLAGS_BOT_DEFAULT_COMMAND: &str = "cargo run --release";

// ── Output types ─────────────────────────────────────────────────────

/// Where the runtime seed comes from. `Generated` is only produced on a
/// non-resume path; `FromStoreOnResume` defers seed realization to the
/// execution layer (which reads it off the stored tournament row).
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SeedSource {
    Explicit(u64),
    Generated(u64),
    FromStoreOnResume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatChoice {
    RoundRobin,
    Gauntlet {
        challenger: String,
        opponents: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTiming {
    pub move_timeout_ms: u32,
    pub preprocessing_timeout_ms: u32,
    pub startup_timeout_ms: u32,
    pub configure_timeout_ms: u32,
    pub network_grace_ms: u32,
}

/// All decisions resolved. Carries enough information to execute the
/// tournament *and* (via the projection in Chunk 6) round-trip back to
/// a `TournamentConfig` for `--save-as`.
pub struct ResolvedRun {
    pub players: Vec<ResolvedPlayer>,
    pub game: ResolvedGameChoice,
    pub timing: ResolvedTiming,
    pub format: FormatChoice,
    pub target_games_per_matchup: u32,
    pub max_failures_per_pair: u32,
    pub max_parallel: u32,
    pub seed: SeedSource,
    pub store_path: PathBuf,
    pub replay_dir: Option<PathBuf>,
    pub anchor: String,
    pub anchor_elo: f64,
    pub results_json: Option<PathBuf>,
    pub save_as: Option<PathBuf>,
    pub resume: Option<TournamentId>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("failed to read config file {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse config file: {0}")]
    ConfigParse(#[from] toml::de::Error),
    #[error("{0}")]
    Validation(String),
}

impl ResolveError {
    fn v(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }
}

// ── Loaded-config helper ─────────────────────────────────────────────

pub struct LoadedConfig {
    pub config: TournamentConfig,
    /// Absolute parent directory of the config file; used to resolve
    /// config-internal relative paths.
    pub dir: PathBuf,
    /// File stem of the config; used to derive the default `store_path`.
    pub stem: Option<String>,
}

pub fn load_config(path: &Path) -> Result<LoadedConfig, ResolveError> {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let abs = absolutize(path, &cwd);
    let raw = fs::read_to_string(&abs).map_err(|source| ResolveError::ConfigRead {
        path: abs.clone(),
        source,
    })?;
    let config: TournamentConfig = toml::from_str(&raw)?;
    let dir = abs
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = abs.file_stem().and_then(|s| s.to_str()).map(String::from);
    Ok(LoadedConfig { config, dir, stem })
}

// ── Resolve entry points ─────────────────────────────────────────────

pub fn resolve(
    args: RunArgs,
    seed_gen: &mut dyn FnMut() -> u64,
) -> Result<ResolvedRun, ResolveError> {
    let loaded = match args.config.as_deref() {
        Some(p) => Some(load_config(p)?),
        None => None,
    };
    resolve_loaded(args, loaded, seed_gen)
}

pub fn resolve_loaded(
    args: RunArgs,
    loaded: Option<LoadedConfig>,
    seed_gen: &mut dyn FnMut() -> u64,
) -> Result<ResolvedRun, ResolveError> {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (cfg, config_dir, config_stem) = match loaded {
        Some(LoadedConfig { config, dir, stem }) => (config, Some(dir), stem),
        None => (TournamentConfig::default(), None, None),
    };

    let players = resolve_players(&args.bots, &cfg.players, config_dir.as_deref(), &cwd)?;
    validate_players(&players)?;
    let format = resolve_format_choice(&args, &cfg, &players)?;

    let target_games_per_matchup = positive(
        args.games
            .or(cfg.target_games_per_matchup)
            .unwrap_or(DEFAULT_GAMES),
        "target_games_per_matchup",
    )?;
    let max_failures_per_pair = positive(
        args.max_failures
            .or(cfg.max_failures_per_pair)
            .unwrap_or(DEFAULT_MAX_FAILURES),
        "max_failures_per_pair",
    )?;
    let max_parallel = positive(
        args.max_parallel
            .or(cfg.max_parallel)
            .unwrap_or(DEFAULT_MAX_PARALLEL),
        "max_parallel",
    )?;

    let game = resolve_game(&args, cfg.game.as_ref())?;

    let cfg_timing = cfg.timing.clone().unwrap_or_default();
    let timing = ResolvedTiming {
        move_timeout_ms: args
            .move_timeout_ms
            .or(cfg_timing.move_timeout_ms)
            .unwrap_or(DEFAULT_MOVE_TIMEOUT_MS),
        preprocessing_timeout_ms: args
            .preprocessing_timeout_ms
            .or(cfg_timing.preprocessing_timeout_ms)
            .unwrap_or(DEFAULT_PREP_TIMEOUT_MS),
        startup_timeout_ms: args
            .startup_timeout_ms
            .or(cfg_timing.startup_timeout_ms)
            .unwrap_or(DEFAULT_STARTUP_TIMEOUT_MS),
        configure_timeout_ms: args
            .configure_timeout_ms
            .or(cfg_timing.configure_timeout_ms)
            .unwrap_or(DEFAULT_CONFIGURE_TIMEOUT_MS),
        network_grace_ms: args
            .network_grace_ms
            .or(cfg_timing.network_grace_ms)
            .unwrap_or(DEFAULT_NETWORK_GRACE_MS),
    };

    let resume = args.resume.map(TournamentId);
    let explicit_seed = args.seed.or(cfg.seed);
    let seed = resolve_seed(explicit_seed, resume.is_some(), seed_gen)?;

    let (anchor, anchor_elo) = resolve_elo_inputs(&args, &cfg, &players, &format)?;

    let store_path = resolve_store_path(
        args.store_path.as_deref(),
        cfg.store_path.as_deref(),
        config_dir.as_deref(),
        config_stem.as_deref(),
        &cwd,
    );
    let replay_dir = args
        .replay_dir
        .as_deref()
        .map(|p| absolutize(p, &cwd))
        .or_else(|| {
            cfg.replay_dir
                .as_deref()
                .map(|p| absolutize(p, config_dir.as_deref().unwrap_or(&cwd)))
        });
    let results_json = args.results_json.as_deref().map(|p| absolutize(p, &cwd));
    let save_as = args.save_as.as_deref().map(|p| absolutize(p, &cwd));

    Ok(ResolvedRun {
        players,
        game,
        timing,
        format,
        target_games_per_matchup,
        max_failures_per_pair,
        max_parallel,
        seed,
        store_path,
        replay_dir,
        anchor,
        anchor_elo,
        results_json,
        save_as,
        resume,
    })
}

// ── Field-level resolvers ────────────────────────────────────────────

fn resolve_format_choice(
    args: &RunArgs,
    cfg: &TournamentConfig,
    players: &[ResolvedPlayer],
) -> Result<FormatChoice, ResolveError> {
    let raw = args
        .format
        .as_deref()
        .or(cfg.format.as_deref())
        .unwrap_or("round_robin")
        .replace('-', "_");
    match raw.as_str() {
        "round_robin" => {
            if players.len() < 2 {
                return Err(ResolveError::v("round_robin requires at least 2 players"));
            }
            return Ok(FormatChoice::RoundRobin);
        },
        "gauntlet" => {},
        other => {
            return Err(ResolveError::v(format!(
                "unknown format `{other}` (expected `round_robin` or `gauntlet`)"
            )))
        },
    }
    let challenger = args
        .challenger
        .clone()
        .or_else(|| cfg.gauntlet.as_ref().map(|g| g.challenger.clone()))
        .ok_or_else(|| {
            ResolveError::v("gauntlet format requires --challenger or [gauntlet].challenger")
        })?;
    let opponents = if !args.opponents.is_empty() {
        args.opponents.clone()
    } else {
        cfg.gauntlet
            .as_ref()
            .map(|g| g.opponents.clone())
            .unwrap_or_default()
    };
    if opponents.is_empty() {
        return Err(ResolveError::v(
            "gauntlet format requires at least one --opponent or [gauntlet].opponents",
        ));
    }
    let player_ids: HashSet<&str> = players.iter().map(|p| p.id.as_str()).collect();
    if !player_ids.contains(challenger.as_str()) {
        return Err(ResolveError::v(format!(
            "gauntlet challenger `{challenger}` is not in the player list"
        )));
    }
    let mut seen_opp = HashSet::new();
    for opp in &opponents {
        if opp == &challenger {
            return Err(ResolveError::v(format!(
                "gauntlet opponent `{opp}` is the challenger"
            )));
        }
        if !player_ids.contains(opp.as_str()) {
            return Err(ResolveError::v(format!(
                "gauntlet opponent `{opp}` is not in the player list"
            )));
        }
        if !seen_opp.insert(opp.as_str()) {
            return Err(ResolveError::v(format!(
                "duplicate gauntlet opponent `{opp}`"
            )));
        }
    }
    Ok(FormatChoice::Gauntlet {
        challenger,
        opponents,
    })
}

fn resolve_players(
    bots: &[BotArg],
    cfg_players: &[PlayerEntry],
    config_dir: Option<&Path>,
    cwd: &Path,
) -> Result<Vec<ResolvedPlayer>, ResolveError> {
    if !bots.is_empty() {
        Ok(bots.iter().map(|b| flag_bot_to_player(b, cwd)).collect())
    } else {
        Ok(cfg_players
            .iter()
            .map(|p| config_entry_to_player(p, config_dir.unwrap_or(cwd)))
            .collect())
    }
}

fn flag_bot_to_player(bot: &BotArg, cwd: &Path) -> ResolvedPlayer {
    let working_dir = absolutize(&bot.working_dir, cwd);
    ResolvedPlayer {
        id: bot.id.clone(),
        spec: PlayerSpec::Subprocess {
            agent_id: bot.id.clone(),
            command: FLAGS_BOT_DEFAULT_COMMAND.into(),
            working_dir: Some(working_dir),
        },
    }
}

fn config_entry_to_player(entry: &PlayerEntry, config_dir: &Path) -> ResolvedPlayer {
    let working_dir = entry
        .working_dir
        .as_ref()
        .map(|p| absolutize(p, config_dir));
    ResolvedPlayer {
        id: entry.id.clone(),
        spec: PlayerSpec::Subprocess {
            agent_id: entry.id.clone(),
            command: entry.command.clone(),
            working_dir,
        },
    }
}

fn validate_players(players: &[ResolvedPlayer]) -> Result<(), ResolveError> {
    if players.is_empty() {
        return Err(ResolveError::v(
            "no players given (use --bot id=working_dir flags or [[players]] in TOML)",
        ));
    }
    let mut seen = HashSet::new();
    for p in players {
        if !seen.insert(p.id.as_str()) {
            return Err(ResolveError::v(format!("duplicate player id `{}`", p.id)));
        }
    }
    Ok(())
}

fn positive(value: u32, field: &str) -> Result<u32, ResolveError> {
    if value == 0 {
        Err(ResolveError::v(format!("{field} must be > 0")))
    } else {
        Ok(value)
    }
}

fn resolve_game(
    args: &RunArgs,
    cfg: Option<&GameSection>,
) -> Result<ResolvedGameChoice, ResolveError> {
    let empty = GameSection::default();
    let g = cfg.unwrap_or(&empty);
    let preset = args.preset.clone().or_else(|| g.preset.clone());
    let width = args.width.or(g.width);
    let height = args.height.or(g.height);
    let cheese = args.cheese.or(g.cheese);
    let symmetric = args.symmetric.or(g.symmetric);
    let max_turns = args
        .max_turns
        .or_else(|| g.max_turns.and_then(std::num::NonZeroU16::new));

    let has_preset = preset.is_some();
    let has_any_dim = width.is_some() || height.is_some() || cheese.is_some();
    let has_all_dims = width.is_some() && height.is_some() && cheese.is_some();

    match (has_preset, has_any_dim) {
        (true, true) => Err(ResolveError::v(
            "game config: use either `preset` or (width, height, cheese), not both",
        )),
        (false, false) => Err(ResolveError::v(
            "game config: set `preset` or all of (width, height, cheese)",
        )),
        (true, false) => {
            if symmetric.is_some() {
                return Err(ResolveError::v(
                    "game config: `symmetric` is only valid with custom (width, height, cheese)",
                ));
            }
            Ok(ResolvedGameChoice::Preset {
                name: preset.unwrap(),
                max_turns_override: max_turns,
            })
        },
        (false, true) => {
            if !has_all_dims {
                return Err(ResolveError::v(
                    "game config: provide all of (width, height, cheese) or use `preset`",
                ));
            }
            Ok(ResolvedGameChoice::Custom {
                width: width.unwrap(),
                height: height.unwrap(),
                cheese: cheese.unwrap(),
                symmetric: symmetric.unwrap_or(true),
                max_turns,
            })
        },
    }
}

fn resolve_seed(
    explicit: Option<u64>,
    has_resume: bool,
    seed_gen: &mut dyn FnMut() -> u64,
) -> Result<SeedSource, ResolveError> {
    match (explicit, has_resume) {
        (Some(s), _) => {
            if s > i64::MAX as u64 {
                return Err(ResolveError::v(format!("seed {s} exceeds i64::MAX")));
            }
            Ok(SeedSource::Explicit(s))
        },
        (None, true) => Ok(SeedSource::FromStoreOnResume),
        (None, false) => Ok(SeedSource::Generated(seed_gen())),
    }
}

fn resolve_elo_inputs(
    args: &RunArgs,
    cfg: &TournamentConfig,
    players: &[ResolvedPlayer],
    format: &FormatChoice,
) -> Result<(String, f64), ResolveError> {
    let cfg_elo = cfg.elo.clone().unwrap_or_default();
    let anchor = args
        .anchor
        .clone()
        .or(cfg_elo.anchor)
        .unwrap_or_else(|| match format {
            FormatChoice::Gauntlet { challenger, .. } => challenger.clone(),
            FormatChoice::RoundRobin => players[0].id.clone(),
        });
    let player_ids: HashSet<&str> = players.iter().map(|p| p.id.as_str()).collect();
    if !player_ids.contains(anchor.as_str()) {
        return Err(ResolveError::v(format!(
            "elo anchor `{anchor}` is not in the player list"
        )));
    }
    let anchor_elo = args
        .anchor_elo
        .or(cfg_elo.anchor_elo)
        .unwrap_or(DEFAULT_ANCHOR_ELO);
    Ok((anchor, anchor_elo))
}

fn resolve_store_path(
    flag: Option<&Path>,
    cfg: Option<&Path>,
    config_dir: Option<&Path>,
    config_stem: Option<&str>,
    cwd: &Path,
) -> PathBuf {
    if let Some(p) = flag {
        return absolutize(p, cwd);
    }
    if let Some(p) = cfg {
        let base = config_dir.unwrap_or(cwd);
        return absolutize(p, base);
    }
    if let (Some(dir), Some(stem)) = (config_dir, config_stem) {
        return dir.join(format!("{stem}.db"));
    }
    cwd.join(DEFAULT_STORE_FILENAME)
}

// ── Path helper ──────────────────────────────────────────────────────

/// Make `path` absolute relative to `base`. Does not touch the filesystem.
fn absolutize(path: &Path, base: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tournament_config::{EloSection, GauntletSection, TimingSection};

    fn expect_err(result: Result<ResolvedRun, ResolveError>) -> ResolveError {
        match result {
            Ok(_) => panic!("expected error, got Ok"),
            Err(e) => e,
        }
    }

    fn empty_args() -> RunArgs {
        RunArgs {
            bots: vec![],
            format: None,
            games: None,
            max_failures: None,
            max_parallel: None,
            seed: None,
            config: None,
            save_as: None,
            resume: None,
            results_json: None,
            store_path: None,
            replay_dir: None,
            preset: None,
            width: None,
            height: None,
            cheese: None,
            symmetric: None,
            max_turns: None,
            move_timeout_ms: None,
            preprocessing_timeout_ms: None,
            startup_timeout_ms: None,
            configure_timeout_ms: None,
            network_grace_ms: None,
            challenger: None,
            opponents: vec![],
            anchor: None,
            anchor_elo: None,
        }
    }

    fn args_with_two_bots() -> RunArgs {
        let mut a = empty_args();
        a.bots = vec![
            BotArg {
                id: "alpha".into(),
                working_dir: "alpha-dir".into(),
            },
            BotArg {
                id: "beta".into(),
                working_dir: "beta-dir".into(),
            },
        ];
        a.preset = Some("tiny".into());
        a
    }

    fn fixed_seed_gen(value: u64) -> impl FnMut() -> u64 {
        let mut emitted = false;
        move || {
            assert!(!emitted, "seed_gen called more than once");
            emitted = true;
            value
        }
    }

    #[test]
    fn defaults_applied_when_neither_config_nor_flag_set() {
        let args = args_with_two_bots();
        let mut gen = fixed_seed_gen(42);
        let resolved = resolve_loaded(args, None, &mut gen).expect("resolve");
        assert_eq!(resolved.target_games_per_matchup, DEFAULT_GAMES);
        assert_eq!(resolved.max_failures_per_pair, DEFAULT_MAX_FAILURES);
        assert_eq!(resolved.max_parallel, DEFAULT_MAX_PARALLEL);
        assert_eq!(resolved.timing.move_timeout_ms, DEFAULT_MOVE_TIMEOUT_MS);
        assert_eq!(
            resolved.timing.preprocessing_timeout_ms,
            DEFAULT_PREP_TIMEOUT_MS
        );
        assert_eq!(
            resolved.timing.startup_timeout_ms,
            DEFAULT_STARTUP_TIMEOUT_MS
        );
        assert_eq!(
            resolved.timing.configure_timeout_ms,
            DEFAULT_CONFIGURE_TIMEOUT_MS
        );
        assert_eq!(resolved.timing.network_grace_ms, DEFAULT_NETWORK_GRACE_MS);
        assert_eq!(resolved.format, FormatChoice::RoundRobin);
        match resolved.seed {
            SeedSource::Generated(42) => {},
            other => panic!("expected Generated(42), got {other:?}"),
        }
    }

    #[test]
    fn config_values_used_when_flags_absent() {
        let cfg = TournamentConfig {
            target_games_per_matchup: Some(11),
            max_parallel: Some(4),
            format: Some("round_robin".into()),
            game: Some(GameSection {
                preset: Some("tiny".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let loaded = Some(LoadedConfig {
            config: cfg,
            dir: PathBuf::from("/tmp"),
            stem: Some("ladder".into()),
        });
        let args = args_with_two_bots();
        let mut gen = fixed_seed_gen(7);
        let resolved = resolve_loaded(args, loaded, &mut gen).expect("resolve");
        assert_eq!(resolved.target_games_per_matchup, 11);
        assert_eq!(resolved.max_parallel, 4);
    }

    #[test]
    fn flag_overrides_config() {
        let cfg = TournamentConfig {
            target_games_per_matchup: Some(11),
            ..Default::default()
        };
        let loaded = Some(LoadedConfig {
            config: cfg,
            dir: PathBuf::from("/tmp"),
            stem: Some("ladder".into()),
        });
        let mut args = args_with_two_bots();
        args.games = Some(99);
        let mut gen = fixed_seed_gen(0);
        let resolved = resolve_loaded(args, loaded, &mut gen).expect("resolve");
        assert_eq!(resolved.target_games_per_matchup, 99);
    }

    #[test]
    fn explicit_seed_flag_wins_over_config_and_generator() {
        let cfg = TournamentConfig {
            seed: Some(123),
            ..Default::default()
        };
        let loaded = Some(LoadedConfig {
            config: cfg,
            dir: PathBuf::from("/tmp"),
            stem: Some("ladder".into()),
        });
        let mut args = args_with_two_bots();
        args.seed = Some(456);
        let mut never_called = || panic!("seed_gen called when explicit seed present");
        let resolved = resolve_loaded(args, loaded, &mut never_called).expect("resolve");
        assert_eq!(resolved.seed, SeedSource::Explicit(456));
    }

    #[test]
    fn config_seed_used_when_flag_absent() {
        let cfg = TournamentConfig {
            seed: Some(123),
            ..Default::default()
        };
        let loaded = Some(LoadedConfig {
            config: cfg,
            dir: PathBuf::from("/tmp"),
            stem: Some("ladder".into()),
        });
        let args = args_with_two_bots();
        let mut never_called = || panic!("seed_gen called when explicit seed present");
        let resolved = resolve_loaded(args, loaded, &mut never_called).expect("resolve");
        assert_eq!(resolved.seed, SeedSource::Explicit(123));
    }

    #[test]
    fn resume_without_explicit_seed_defers_to_store() {
        let mut args = args_with_two_bots();
        args.resume = Some(7);
        let mut never_called = || panic!("seed_gen called on resume path");
        let resolved = resolve_loaded(args, None, &mut never_called).expect("resolve");
        assert_eq!(resolved.seed, SeedSource::FromStoreOnResume);
        assert_eq!(resolved.resume, Some(TournamentId(7)));
    }

    #[test]
    fn resume_with_explicit_seed_keeps_explicit() {
        let mut args = args_with_two_bots();
        args.resume = Some(7);
        args.seed = Some(99);
        let mut never_called = || panic!("seed_gen called with explicit seed present");
        let resolved = resolve_loaded(args, None, &mut never_called).expect("resolve");
        assert_eq!(resolved.seed, SeedSource::Explicit(99));
    }

    #[test]
    fn no_resume_no_explicit_seed_generates() {
        let args = args_with_two_bots();
        let mut gen = fixed_seed_gen(0xC0FFEE);
        let resolved = resolve_loaded(args, None, &mut gen).expect("resolve");
        assert_eq!(resolved.seed, SeedSource::Generated(0xC0FFEE));
    }

    #[test]
    fn seed_out_of_range_rejected() {
        let mut args = args_with_two_bots();
        args.seed = Some(u64::MAX);
        let mut g = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut g));
        assert!(err.to_string().contains("seed"), "got: {err}");
    }

    #[test]
    fn format_alias_round_dash_normalized() {
        let mut args = args_with_two_bots();
        args.format = Some("round-robin".into());
        let mut gen = fixed_seed_gen(0);
        let resolved = resolve_loaded(args, None, &mut gen).expect("resolve");
        assert_eq!(resolved.format, FormatChoice::RoundRobin);
    }

    #[test]
    fn unknown_format_rejected() {
        let mut args = args_with_two_bots();
        args.format = Some("swiss".into());
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(err.to_string().contains("unknown format"), "got: {err}");
    }

    #[test]
    fn round_robin_requires_two_players() {
        let mut args = empty_args();
        args.bots = vec![BotArg {
            id: "solo".into(),
            working_dir: "x".into(),
        }];
        args.preset = Some("tiny".into());
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(err.to_string().contains("at least 2 players"), "got: {err}");
    }

    #[test]
    fn duplicate_player_id_rejected() {
        let mut args = args_with_two_bots();
        args.bots[1].id = "alpha".into();
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(
            err.to_string().contains("duplicate player id"),
            "got: {err}"
        );
    }

    #[test]
    fn game_config_both_preset_and_dims_rejected() {
        let mut args = args_with_two_bots();
        args.width = Some(7);
        args.height = Some(7);
        args.cheese = Some(3);
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(
            err.to_string().contains("not both") || err.to_string().contains("preset"),
            "got: {err}"
        );
    }

    #[test]
    fn game_config_neither_preset_nor_dims_rejected() {
        let mut args = args_with_two_bots();
        args.preset = None;
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(
            err.to_string().contains("preset") || err.to_string().contains("width"),
            "got: {err}"
        );
    }

    #[test]
    fn game_config_symmetric_with_preset_rejected() {
        let mut args = args_with_two_bots();
        args.symmetric = Some(false);
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(err.to_string().contains("symmetric"), "got: {err}");
    }

    #[test]
    fn game_config_custom_round_trips_through_resolve() {
        let mut args = args_with_two_bots();
        args.preset = None;
        args.width = Some(7);
        args.height = Some(7);
        args.cheese = Some(5);
        args.symmetric = Some(true);
        let mut gen = fixed_seed_gen(0);
        let resolved = resolve_loaded(args, None, &mut gen).expect("resolve");
        match resolved.game {
            ResolvedGameChoice::Custom {
                width: 7,
                height: 7,
                cheese: 5,
                symmetric: true,
                max_turns: None,
            } => {},
            other => panic!("unexpected game: {other:?}"),
        }
    }

    #[test]
    fn anchor_defaults_to_first_player_in_round_robin() {
        let args = args_with_two_bots();
        let mut gen = fixed_seed_gen(0);
        let resolved = resolve_loaded(args, None, &mut gen).expect("resolve");
        assert_eq!(resolved.anchor, "alpha");
        assert_eq!(resolved.anchor_elo, DEFAULT_ANCHOR_ELO);
    }

    #[test]
    fn anchor_defaults_to_challenger_in_gauntlet() {
        let mut args = args_with_two_bots();
        args.format = Some("gauntlet".into());
        args.challenger = Some("beta".into());
        args.opponents = vec!["alpha".into()];
        let mut gen = fixed_seed_gen(0);
        let resolved = resolve_loaded(args, None, &mut gen).expect("resolve");
        assert_eq!(resolved.anchor, "beta");
    }

    #[test]
    fn anchor_not_in_players_rejected() {
        let mut args = args_with_two_bots();
        args.anchor = Some("ghost".into());
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(err.to_string().contains("anchor"), "got: {err}");
    }

    #[test]
    fn gauntlet_missing_challenger_rejected() {
        let mut args = args_with_two_bots();
        args.format = Some("gauntlet".into());
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(err.to_string().contains("challenger"), "got: {err}");
    }

    #[test]
    fn gauntlet_challenger_in_opponents_rejected() {
        let mut args = args_with_two_bots();
        args.format = Some("gauntlet".into());
        args.challenger = Some("alpha".into());
        args.opponents = vec!["alpha".into()];
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(err.to_string().contains("is the challenger"), "got: {err}");
    }

    #[test]
    fn gauntlet_duplicate_opponents_rejected() {
        let mut args = args_with_two_bots();
        args.bots.push(BotArg {
            id: "gamma".into(),
            working_dir: "g".into(),
        });
        args.format = Some("gauntlet".into());
        args.challenger = Some("alpha".into());
        args.opponents = vec!["beta".into(), "beta".into()];
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(err.to_string().contains("duplicate"), "got: {err}");
    }

    #[test]
    fn gauntlet_unknown_opponent_rejected() {
        let mut args = args_with_two_bots();
        args.format = Some("gauntlet".into());
        args.challenger = Some("alpha".into());
        args.opponents = vec!["ghost".into()];
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(err.to_string().contains("ghost"), "got: {err}");
    }

    #[test]
    fn zero_target_games_rejected() {
        let mut args = args_with_two_bots();
        args.games = Some(0);
        let mut gen = fixed_seed_gen(0);
        let err = expect_err(resolve_loaded(args, None, &mut gen));
        assert!(
            err.to_string().contains("target_games_per_matchup"),
            "got: {err}"
        );
    }

    #[test]
    fn store_path_defaults_to_config_stem_db_next_to_config() {
        let cfg = TournamentConfig::default();
        let loaded = Some(LoadedConfig {
            config: cfg,
            dir: PathBuf::from("/tmp/eval"),
            stem: Some("ladder".into()),
        });
        let args = args_with_two_bots();
        let mut gen = fixed_seed_gen(0);
        let resolved = resolve_loaded(args, loaded, &mut gen).expect("resolve");
        assert_eq!(resolved.store_path, PathBuf::from("/tmp/eval/ladder.db"));
    }

    #[test]
    fn store_path_defaults_to_ratings_db_in_cwd_without_config() {
        let args = args_with_two_bots();
        let mut gen = fixed_seed_gen(0);
        let resolved = resolve_loaded(args, None, &mut gen).expect("resolve");
        assert!(
            resolved.store_path.ends_with(DEFAULT_STORE_FILENAME),
            "store_path = {:?}",
            resolved.store_path
        );
    }

    #[test]
    fn flag_store_path_resolves_from_cwd() {
        let mut args = args_with_two_bots();
        args.store_path = Some(PathBuf::from("custom.db"));
        let cfg = TournamentConfig::default();
        let loaded = Some(LoadedConfig {
            config: cfg,
            dir: PathBuf::from("/tmp/eval"),
            stem: Some("ladder".into()),
        });
        let mut gen = fixed_seed_gen(0);
        let resolved = resolve_loaded(args, loaded, &mut gen).expect("resolve");
        assert!(
            resolved.store_path.ends_with("custom.db"),
            "store_path = {:?}",
            resolved.store_path
        );
        assert!(!resolved.store_path.starts_with("/tmp/eval"));
    }

    #[test]
    fn config_store_path_resolves_relative_to_config_dir() {
        let cfg = TournamentConfig {
            store_path: Some(PathBuf::from("ratings/db.sqlite")),
            ..Default::default()
        };
        let loaded = Some(LoadedConfig {
            config: cfg,
            dir: PathBuf::from("/tmp/eval"),
            stem: Some("ladder".into()),
        });
        let args = args_with_two_bots();
        let mut gen = fixed_seed_gen(0);
        let resolved = resolve_loaded(args, loaded, &mut gen).expect("resolve");
        assert_eq!(
            resolved.store_path,
            PathBuf::from("/tmp/eval/ratings/db.sqlite")
        );
    }

    #[test]
    fn explicit_seed_unused_when_already_within_bounds() {
        // Sanity: i64::MAX should be accepted; one over should be rejected.
        let mut args = args_with_two_bots();
        args.seed = Some(i64::MAX as u64);
        let mut g = fixed_seed_gen(0);
        let resolved = resolve_loaded(args, None, &mut g).expect("resolve");
        assert_eq!(resolved.seed, SeedSource::Explicit(i64::MAX as u64));
    }

    // Silence unused-import warning on EloSection, GauntletSection, TimingSection
    // — they're held here so the tests above can construct them when adding
    // coverage; not all are wired yet.
    #[allow(dead_code)]
    fn _unused_silencers() -> (EloSection, GauntletSection, TimingSection) {
        (
            EloSection::default(),
            GauntletSection {
                challenger: String::new(),
                opponents: vec![],
            },
            TimingSection::default(),
        )
    }
}
