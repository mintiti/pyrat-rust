//! End-to-end subprocess tests for `pyrat-eval tournament run`.
//!
//! Two kinds of tests live here:
//! - TOML-driven tests that point `command` at the in-crate `test-bot`
//!   binary explicitly. Covers `--config`, `--save-as`, `--resume`,
//!   game-config drift on resume, and pre-bootstrap validation.
//! - The flags-only e2e (`flags_only_runs_through`) that exercises
//!   `--bot id=working_dir` shorthand. Because the shorthand defaults
//!   the command to `cargo run --release`, pointing it at `eval/session`
//!   would spawn `pyrat-eval` itself (the crate's `default-run`); the
//!   test points at the isolated-workspace fixture crates under
//!   `tests/fixtures/{bot-a,bot-b}/` instead.

use std::path::Path;
use std::process::Command;

fn eval_bin() -> &'static str {
    env!("CARGO_BIN_EXE_pyrat-eval")
}

fn test_bot_bin() -> &'static str {
    env!("CARGO_BIN_EXE_test-bot")
}

/// Minimal TOML pinning a round-robin between two `test-bot`s on a tiny
/// board. Callers can pass a custom `extra` block (e.g. `seed = 42`).
fn minimal_toml(extra: &str) -> String {
    let bot = test_bot_bin();
    format!(
        r#"format = "round_robin"
target_games_per_matchup = 1
max_parallel = 1
{extra}

[game]
preset = "tiny"
max_turns = 10

[timing]
move_timeout_ms = 2000
preprocessing_timeout_ms = 5000
startup_timeout_ms = 10000
configure_timeout_ms = 3000

[[players]]
id = "alice"
command = "{bot}"

[[players]]
id = "bob"
command = "{bot}"
"#
    )
}

fn write_toml(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write toml");
}

/// Spawn `pyrat-eval tournament run` with the given args. Returns the
/// raw output (stdout/stderr/status). Inherits `PYRAT_TEST_BOT_SEED=1`
/// so the action stream is deterministic.
fn run_tournament(args: &[&str]) -> std::process::Output {
    Command::new(eval_bin())
        .env("PYRAT_TEST_BOT_SEED", "1")
        .arg("tournament")
        .arg("run")
        .args(args)
        .output()
        .expect("spawn pyrat-eval")
}

fn assert_success(out: &std::process::Output, args: &[&str]) {
    assert!(
        out.status.success(),
        "pyrat-eval exited non-zero.\nargs: {args:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Absolute path to a fixture bot crate. Built as an isolated
/// `[workspace]` so it doesn't collide with `pyrat-eval`'s `default-run`
/// — when the CLI's `--bot id=working_dir` shorthand defaults the
/// command to `cargo run --release`, Cargo picks `fixture-bot-{a,b}`
/// from the local Cargo.toml's `[[bin]]`.
fn fixture_bot_dir(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

// ── Smoke tests ──────────────────────────────────────────────────────

#[test]
fn minimal_toml_round_robin_runs_through() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("ladder.toml");
    let store_path = tmp.path().join("ratings.db");
    let toml = minimal_toml(&format!(
        "store_path = {:?}\nseed = 7",
        store_path.to_string_lossy()
    ));
    write_toml(&cfg_path, &toml);

    let args = ["--config", cfg_path.to_str().unwrap()];
    let out = run_tournament(&args);
    assert_success(&out, &args);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Tournament") && stdout.contains("finished"),
        "stdout should mention tournament status.\nstdout: {stdout}"
    );
}

/// Pins the README one-liner: flags-only invocation without `--config`,
/// `--bot id=working_dir` shorthand against bot crates outside the root
/// workspace. Defaults to `--preset tiny` per Chunk 1.
#[test]
fn flags_only_runs_through() {
    let bot_a = format!("alpha={}", fixture_bot_dir("bot-a"));
    let bot_b = format!("beta={}", fixture_bot_dir("bot-b"));
    let tmp = tempfile::tempdir().unwrap();
    let store_path = tmp.path().join("ratings.db");
    let store_path_str = store_path.to_str().unwrap();

    let args = [
        "--bot",
        &bot_a,
        "--bot",
        &bot_b,
        "--games",
        "1",
        "--seed",
        "7",
        "--store-path",
        store_path_str,
    ];
    let out = run_tournament(&args);
    assert_success(&out, &args);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Tournament") && stdout.contains("finished"),
        "stdout should mention tournament status: {stdout}"
    );
}

// ── Validation ───────────────────────────────────────────────────────

#[test]
fn invalid_config_fails_before_spawn() {
    // Duplicate player ids in the TOML — the resolver catches this
    // before any bot launches. Sentinel: the default store file
    // (<cfg_stem>.db next to the config) should not be created — we
    // stop before EvalStore::open.
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("dup.toml");
    let bot = test_bot_bin();
    let toml = format!(
        r#"format = "round_robin"
target_games_per_matchup = 1

[game]
preset = "tiny"

[[players]]
id = "same"
command = "{bot}"

[[players]]
id = "same"
command = "{bot}"
"#
    );
    write_toml(&cfg_path, &toml);

    let out = run_tournament(&["--config", cfg_path.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "expected non-zero exit on duplicate player id"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("duplicate player id"),
        "stderr should mention duplicate player id: {stderr}"
    );
    // Sentinel: no store file materialized.
    let default_store = tmp.path().join("dup.db");
    assert!(
        !default_store.exists(),
        "store file should not exist after resolver-rejected config: {default_store:?}",
    );
}

/// An invalid game config (e.g. too much cheese for the board) must
/// surface before `bootstrap_new` commits a tournament row. Otherwise a
/// failed validation leaves a dangling tournament behind that will never
/// see any attempts.
#[test]
fn invalid_game_config_fails_before_tournament_row() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("ladder.toml");
    let store_path = tmp.path().join("ratings.db");
    let bot = test_bot_bin();
    // 3x3 board with 100 cheese: the engine refuses to populate it.
    let toml = format!(
        r#"format = "round_robin"
target_games_per_matchup = 1
max_parallel = 1
store_path = {store:?}
seed = 7

[game]
width = 3
height = 3
cheese = 100

[timing]
move_timeout_ms = 2000
preprocessing_timeout_ms = 5000
startup_timeout_ms = 10000
configure_timeout_ms = 3000

[[players]]
id = "alice"
command = "{bot}"

[[players]]
id = "bob"
command = "{bot}"
"#,
        store = store_path.to_string_lossy(),
    );
    write_toml(&cfg_path, &toml);

    let args = ["--config", cfg_path.to_str().unwrap()];
    let out = run_tournament(&args);
    assert!(
        !out.status.success(),
        "expected non-zero exit for invalid game config"
    );

    // Open the store the CLI would have created (or not) and prove no
    // tournament row leaked.
    let store = pyrat_eval_store::EvalStore::open(&store_path).expect("open store");
    let tournaments = store.list_tournaments().expect("list_tournaments");
    assert!(
        tournaments.is_empty(),
        "tournament row leaked after pre-bootstrap validation: {tournaments:?}"
    );
}

// ── --save-as ────────────────────────────────────────────────────────

#[test]
fn save_as_omits_implicit_seed() {
    // No --seed flag, no seed in config: the generated seed must not
    // appear in the saved TOML (a saved blueprint is decoupled from
    // any one run's seed).
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("in.toml");
    let store_path = tmp.path().join("ratings.db");
    let toml = minimal_toml(&format!("store_path = {:?}", store_path.to_string_lossy()));
    write_toml(&cfg_path, &toml);
    let save_path = tmp.path().join("out.toml");

    let args = [
        "--config",
        cfg_path.to_str().unwrap(),
        "--save-as",
        save_path.to_str().unwrap(),
    ];
    let out = run_tournament(&args);
    assert_success(&out, &args);

    let saved = std::fs::read_to_string(&save_path).expect("read saved toml");
    assert!(
        !saved.contains("\nseed ="),
        "implicit seed should not appear in saved TOML:\n{saved}"
    );
}

#[test]
fn save_as_keeps_explicit_seed() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("in.toml");
    let store_path = tmp.path().join("ratings.db");
    let toml = minimal_toml(&format!("store_path = {:?}", store_path.to_string_lossy()));
    write_toml(&cfg_path, &toml);
    let save_path = tmp.path().join("out.toml");

    let args = [
        "--config",
        cfg_path.to_str().unwrap(),
        "--seed",
        "42",
        "--save-as",
        save_path.to_str().unwrap(),
    ];
    let out = run_tournament(&args);
    assert_success(&out, &args);

    let saved = std::fs::read_to_string(&save_path).expect("read saved toml");
    assert!(
        saved.contains("seed = 42"),
        "explicit seed should appear in saved TOML:\n{saved}"
    );
}

#[test]
fn save_as_roundtrip() {
    // Write the saved spec, then reload it via --config and run again.
    // The second run must succeed (same store, same blueprint, more games).
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("in.toml");
    let store_path = tmp.path().join("ratings.db");
    let toml = minimal_toml(&format!(
        "store_path = {:?}\nseed = 17",
        store_path.to_string_lossy()
    ));
    write_toml(&cfg_path, &toml);
    let save_path = tmp.path().join("saved.toml");

    let args = [
        "--config",
        cfg_path.to_str().unwrap(),
        "--save-as",
        save_path.to_str().unwrap(),
    ];
    let out = run_tournament(&args);
    assert_success(&out, &args);

    // Second invocation: --config saved.toml. Should not error.
    let args = ["--config", save_path.to_str().unwrap()];
    let out = run_tournament(&args);
    assert_success(&out, &args);
}

// ── Resume ───────────────────────────────────────────────────────────

#[test]
fn resume_with_mismatched_seed_fails_clearly() {
    // Run with --seed 42, find the tournament id by inspecting the
    // store via a second run with mismatched --seed 99 and --resume.
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("ladder.toml");
    let store_path = tmp.path().join("ratings.db");
    let toml = minimal_toml(&format!("store_path = {:?}", store_path.to_string_lossy()));
    write_toml(&cfg_path, &toml);

    // First run with seed 42.
    let args = ["--config", cfg_path.to_str().unwrap(), "--seed", "42"];
    let out = run_tournament(&args);
    assert_success(&out, &args);

    // Resume with a mismatched seed. The tournament id is 1 since this
    // is a fresh store. The CLI must fail before bots launch with a
    // clear seed-mismatch message.
    let args = [
        "--config",
        cfg_path.to_str().unwrap(),
        "--resume",
        "1",
        "--seed",
        "99",
    ];
    let out = run_tournament(&args);
    assert!(
        !out.status.success(),
        "resume with mismatched seed should fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("seed mismatch"),
        "stderr should mention seed mismatch: {stderr}"
    );
}

/// On resume, the planner reuses the stored `game_config_id` (Chunk 5
/// keeps that behavior). But if the user's CLI flags or config resolve
/// to a *different* runtime `GameConfig`, the orchestrator would play
/// matches whose attempts get recorded under a stored row that doesn't
/// describe what was played. The content-hash check rejects this.
#[test]
fn resume_with_drifted_game_config_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("ladder.toml");
    let store_path = tmp.path().join("ratings.db");
    let toml = minimal_toml(&format!(
        "store_path = {:?}\nseed = 42",
        store_path.to_string_lossy()
    ));
    write_toml(&cfg_path, &toml);

    // First run pins the stored game_config_id to a hash of (preset tiny,
    // max_turns = 10). Run is allowed to finish.
    let args = ["--config", cfg_path.to_str().unwrap()];
    let out = run_tournament(&args);
    assert_success(&out, &args);

    // Resume with the same store but a CLI override that drifts the
    // runtime config (max_turns now 99). The planner is still built
    // with the stored game_config_id (the planner's id-string check
    // passes), but the runtime-config content-hash check trips.
    let args = [
        "--config",
        cfg_path.to_str().unwrap(),
        "--resume",
        "1",
        "--max-turns",
        "99",
    ];
    let out = run_tournament(&args);
    assert!(
        !out.status.success(),
        "resume with drifted game_config should fail.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("game_config"),
        "stderr should mention game_config drift: {stderr}"
    );
}

/// Resume mismatches surface in CLI vocabulary: a drifted --games value
/// is reported as `--games`, not as the library's
/// `target_games_per_matchup`.
#[test]
fn resume_with_drifted_games_names_the_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("ladder.toml");
    let store_path = tmp.path().join("ratings.db");
    let toml = minimal_toml(&format!(
        "store_path = {:?}\nseed = 42",
        store_path.to_string_lossy()
    ));
    write_toml(&cfg_path, &toml);

    // First run: target_games_per_matchup = 1 (from minimal_toml).
    let args = ["--config", cfg_path.to_str().unwrap()];
    let out = run_tournament(&args);
    assert_success(&out, &args);

    // Resume with --games 5 — diverges from the stored target.
    let args = [
        "--config",
        cfg_path.to_str().unwrap(),
        "--resume",
        "1",
        "--games",
        "5",
    ];
    let out = run_tournament(&args);
    assert!(
        !out.status.success(),
        "resume with drifted --games should fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("resume rejected") && stderr.contains("--games"),
        "stderr should name the --games flag: {stderr}"
    );
}

#[test]
fn resume_without_seed_flag_succeeds() {
    // First run with --seed 42, then resume with no --seed flag. Verifies
    // the e2e path: a resume invocation without --seed doesn't error and
    // the stored tournament_seed is preserved. (The deep negative
    // assertion — "no fresh seed was generated" — is pinned at the
    // resolver unit level by `resume_without_explicit_seed_defers_to_store`,
    // which passes a panicking seed_gen.)
    //
    // Keeps --games unchanged across the two invocations; drifting it
    // would trip target_per_pair validation instead of exercising the
    // seed path.
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("ladder.toml");
    let store_path = tmp.path().join("ratings.db");
    let toml = minimal_toml(&format!("store_path = {:?}", store_path.to_string_lossy()));
    write_toml(&cfg_path, &toml);

    let args = ["--config", cfg_path.to_str().unwrap(), "--seed", "42"];
    let out = run_tournament(&args);
    assert_success(&out, &args);

    let args = ["--config", cfg_path.to_str().unwrap(), "--resume", "1"];
    let out = run_tournament(&args);
    assert_success(&out, &args);

    // Verify the stored tournament_seed is still 42 — the resume run
    // didn't overwrite it with a freshly generated value.
    let store = pyrat_eval_store::EvalStore::open(&store_path).expect("open store");
    let tournament = store
        .get_tournament(pyrat_eval_store::TournamentId(1))
        .expect("query tournament")
        .expect("tournament row");
    assert_eq!(
        tournament.tournament_seed, 42,
        "stored tournament_seed should be preserved across resume"
    );
}

// ── Gauntlet ─────────────────────────────────────────────────────────

#[test]
fn gauntlet_runs_through() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("gauntlet.toml");
    let store_path = tmp.path().join("ratings.db");
    let bot = test_bot_bin();
    let toml = format!(
        r#"format = "gauntlet"
target_games_per_matchup = 1
max_parallel = 1
store_path = {store:?}

[game]
preset = "tiny"
max_turns = 10

[timing]
move_timeout_ms = 2000
preprocessing_timeout_ms = 5000
startup_timeout_ms = 10000
configure_timeout_ms = 3000

[[players]]
id = "champ"
command = "{bot}"

[[players]]
id = "rival"
command = "{bot}"

[gauntlet]
challenger = "champ"
opponents = ["rival"]
"#,
        store = store_path.to_string_lossy(),
    );
    write_toml(&cfg_path, &toml);

    let args = ["--config", cfg_path.to_str().unwrap(), "--seed", "5"];
    let out = run_tournament(&args);
    assert_success(&out, &args);
}

// ── Path resolution ──────────────────────────────────────────────────

#[test]
fn config_store_path_resolves_relative_to_config_dir() {
    // Put the config in a sub-directory; reference `store_path =
    // "ratings.db"` relative to it. The store file should land next to
    // the config, not in CWD or the tempdir root.
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let cfg_path = sub.join("ladder.toml");
    let toml = minimal_toml("store_path = \"ratings.db\"\nseed = 9");
    write_toml(&cfg_path, &toml);

    let args = ["--config", cfg_path.to_str().unwrap()];
    let out = run_tournament(&args);
    assert_success(&out, &args);

    let expected_store = sub.join("ratings.db");
    assert!(
        expected_store.exists(),
        "store_path should land next to the config: {expected_store:?}"
    );
}

// ── Results JSON ─────────────────────────────────────────────────────

#[test]
fn results_json_written_when_flag_set() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("ladder.toml");
    let store_path = tmp.path().join("ratings.db");
    let results_path = tmp.path().join("results.json");
    let toml = minimal_toml(&format!(
        "store_path = {:?}\nseed = 11",
        store_path.to_string_lossy()
    ));
    write_toml(&cfg_path, &toml);

    let args = [
        "--config",
        cfg_path.to_str().unwrap(),
        "--results-json",
        results_path.to_str().unwrap(),
    ];
    let out = run_tournament(&args);
    assert_success(&out, &args);

    let raw = std::fs::read_to_string(&results_path).expect("results.json present");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    assert_eq!(parsed["status"], "finished");
    assert!(
        parsed["tournament_id"].as_i64().is_some(),
        "tournament_id present"
    );
    assert!(parsed["standings"].is_array(), "standings is array");
}
