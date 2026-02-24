//! End-to-end subprocess test: runs pyrat-headless with test-bot as both players.

use std::process::Command;

#[test]
fn headless_with_test_bots() {
    let headless_bin = env!("CARGO_BIN_EXE_pyrat-headless");
    let test_bot_bin = env!("CARGO_BIN_EXE_test-bot");

    let output_dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = output_dir.path().join("game_record.json");

    let output = Command::new(headless_bin)
        .arg(test_bot_bin)
        .arg(test_bot_bin)
        .arg("--preset")
        .arg("tiny")
        .arg("--seed")
        .arg("42")
        .arg("--move-timeout-ms")
        .arg("2000")
        .arg("--output")
        .arg(&output_path)
        .output()
        .expect("failed to run pyrat-headless");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "pyrat-headless exited with non-zero status.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Stdout should contain a result line
    assert!(
        stdout.contains("wins!") || stdout.contains("Draw!"),
        "stdout should contain game result.\nstdout: {stdout}"
    );

    // JSON file should exist and be valid
    let json_str = std::fs::read_to_string(&output_path).expect("failed to read game record JSON");
    let record: serde_json::Value =
        serde_json::from_str(&json_str).expect("game record is not valid JSON");

    // Basic structure checks
    assert!(record.get("width").is_some(), "missing 'width' field");
    assert!(record.get("height").is_some(), "missing 'height' field");
    assert!(record.get("turns").is_some(), "missing 'turns' field");
    assert!(record.get("result").is_some(), "missing 'result' field");

    let turns = record["turns"]
        .as_array()
        .expect("'turns' should be an array");
    assert!(!turns.is_empty(), "game should have at least one turn");

    let result = &record["result"];
    assert!(
        result["winner"].as_str().is_some(),
        "result should have a winner string"
    );
    assert!(
        result["turns_played"].as_u64().unwrap() > 0,
        "should have played at least 1 turn"
    );
}
