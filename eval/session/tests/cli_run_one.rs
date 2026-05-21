//! End-to-end subprocess tests: runs `pyrat-eval run-one` with `test-bot` as
//! both players. The bot is seeded via `PYRAT_TEST_BOT_SEED` so action streams
//! are deterministic.
//!
//! These tests pin the legacy GameRecord JSON contract consumed by the GUI
//! replay loader and any downstream scripts. If the shape drifts silently,
//! the GUI breaks; pinning shape + key-presence here catches it.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn eval_bin() -> &'static str {
    env!("CARGO_BIN_EXE_pyrat-eval")
}

fn test_bot_bin() -> &'static str {
    env!("CARGO_BIN_EXE_test-bot")
}

/// Run `pyrat-eval run-one` with `test-bot` as both players. Extra args are
/// appended verbatim. Returns the parsed JSON record from `--output`.
fn run_one(extra_args: &[&str], output_path: &Path) -> Value {
    let bot = test_bot_bin();
    let mut cmd = Command::new(eval_bin());
    cmd.env("PYRAT_TEST_BOT_SEED", "1")
        .arg("run-one")
        .arg(bot)
        .arg(bot)
        .arg("--move-timeout-ms")
        .arg("2000")
        .arg("--output")
        .arg(output_path);
    cmd.args(extra_args);

    let output = cmd.output().expect("failed to run pyrat-eval");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "pyrat-eval exited with non-zero status.\nargs: {extra_args:?}\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("wins!") || stdout.contains("Draw!"),
        "stdout should contain game result.\nstdout: {stdout}"
    );

    let json_str = std::fs::read_to_string(output_path).expect("failed to read game record JSON");
    serde_json::from_str(&json_str).expect("game record is not valid JSON")
}

#[test]
fn seeded_run_pins_legacy_shape() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("game_record.json");
    let record = run_one(
        &["--preset", "tiny", "--seed", "42", "--max-turns", "30"],
        &path,
    );

    // Top-level keys + types.
    assert_eq!(record["width"].as_u64(), Some(11));
    assert_eq!(record["height"].as_u64(), Some(9));
    assert_eq!(record["max_turns"].as_u64(), Some(30));
    assert_eq!(record["seed"].as_u64(), Some(42));

    // Players: exactly two entries, each with the documented fields.
    let players = record["players"].as_array().expect("players array");
    assert_eq!(players.len(), 2);
    for p in players {
        assert!(p["player"].as_str().is_some(), "players[*].player");
        assert!(p["name"].as_str().is_some(), "players[*].name");
        assert!(p["author"].as_str().is_some(), "players[*].author");
        assert!(p["agent_id"].as_str().is_some(), "players[*].agent_id");
    }

    // Turns: at least one entry with every documented field at the right type.
    let turns = record["turns"].as_array().expect("turns array");
    assert!(!turns.is_empty(), "turns should be non-empty");
    let t0 = &turns[0];
    assert!(t0["turn"].as_u64().is_some(), "turn");
    assert!(t0["p1_action"].as_u64().is_some(), "p1_action");
    assert!(t0["p2_action"].as_u64().is_some(), "p2_action");
    assert!(t0["p1_position"].is_array(), "p1_position is [x,y]");
    assert!(t0["p2_position"].is_array(), "p2_position is [x,y]");
    assert!(t0["p1_score"].is_number(), "p1_score");
    assert!(t0["p2_score"].is_number(), "p2_score");
    assert!(
        t0["cheese_remaining"].as_u64().is_some(),
        "cheese_remaining"
    );
    assert!(t0["p1_think_ms"].as_u64().is_some(), "p1_think_ms");
    assert!(t0["p2_think_ms"].as_u64().is_some(), "p2_think_ms");

    // Result: winner is one of the documented labels.
    let winner = record["result"]["winner"]
        .as_str()
        .expect("result.winner string");
    assert!(
        matches!(winner, "Player1" | "Player2" | "Draw"),
        "unexpected winner label: {winner}"
    );
    assert!(
        record["result"]["turns_played"].as_u64().unwrap() > 0,
        "turns_played > 0"
    );
}

#[test]
fn unseeded_run_serializes_numeric_seed() {
    // The PR changed unseeded runs from `"seed": null` to a generated u64.
    // Pin the new contract so it can't drift silently.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("game_record.json");
    let record = run_one(&["--preset", "tiny", "--max-turns", "10"], &path);
    assert!(
        record["seed"].as_u64().is_some(),
        "unseeded run should still serialize a numeric seed, got {:?}",
        record["seed"]
    );
}

#[test]
fn invalid_game_config_fails_before_spawning_bots() {
    // Bad config (1000 cheese on a 4x4 board) should be caught by the
    // pre-flight validation in `run_one`, before `launch_bots` runs. Two
    // sentinel files would be touched if the bot commands were ever
    // executed; assert they never exist.
    let dir = tempfile::tempdir().unwrap();
    let sentinel1 = dir.path().join("spawned-1");
    let sentinel2 = dir.path().join("spawned-2");
    let p1_cmd = format!("touch {} && sleep 30", sentinel1.display());
    let p2_cmd = format!("touch {} && sleep 30", sentinel2.display());

    let output = Command::new(eval_bin())
        .arg("run-one")
        .arg(&p1_cmd)
        .arg(&p2_cmd)
        .arg("--width")
        .arg("4")
        .arg("--height")
        .arg("4")
        .arg("--cheese")
        .arg("1000")
        .output()
        .expect("failed to run pyrat-eval");

    assert!(
        !output.status.success(),
        "pyrat-eval should fail on impossible cheese count"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid game config"),
        "stderr should explain the config failure.\nstderr: {stderr}"
    );
    assert!(
        !sentinel1.exists(),
        "player 1 was spawned before the config was validated"
    );
    assert!(
        !sentinel2.exists(),
        "player 2 was spawned before the config was validated"
    );
}

#[test]
fn preset_max_turns_default_and_override() {
    // --preset tiny without --max-turns keeps the preset's 150.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("default.json");
    let record = run_one(&["--preset", "tiny", "--seed", "1"], &path);
    assert_eq!(
        record["max_turns"].as_u64(),
        Some(150),
        "--preset tiny should keep its 150-turn default when --max-turns is omitted"
    );

    // --preset tiny with --max-turns 30 overrides the preset.
    let path = dir.path().join("override.json");
    let record = run_one(
        &["--preset", "tiny", "--seed", "1", "--max-turns", "30"],
        &path,
    );
    assert_eq!(
        record["max_turns"].as_u64(),
        Some(30),
        "--max-turns should override the preset's value"
    );
}
