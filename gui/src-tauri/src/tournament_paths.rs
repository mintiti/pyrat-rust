//! Where the GUI's eval data lives on disk.
//!
//! One stable store under the app-data dir so every GUI eval lands in the
//! same SQLite the user can point the CLI / alpharat at (`pyrat-eval --store
//! <path> ...`). Replays go in a sibling directory, one subdir per tournament.
//! Both are surfaced in the UI so "shares the CLI's store" is discoverable,
//! not magic. Resolution lives here so a future override (a settings flag)
//! touches one place, not every call site.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// Absolute path to the shared eval SQLite store, `<app_data>/eval.db`.
pub fn store_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("eval.db"))
}

/// Root for per-tournament replay directories, `<app_data>/replays`.
/// Each tournament's `ReplaySink` writes into `replay_root/tournament-<id>/`.
pub fn replay_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("replays"))
}

/// Replay directory for one tournament: `<app_data>/replays/tournament-<id>/`.
pub fn tournament_replay_dir(app: &AppHandle, tournament_id: i64) -> Result<PathBuf, String> {
    Ok(replay_root(app)?.join(format!("tournament-{tournament_id}")))
}

/// The app-data directory, created if missing. Tauri resolves the per-OS
/// location (e.g. `~/Library/Application Support/<bundle-id>` on macOS).
fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create app data dir {}: {e}", dir.display()))?;
    Ok(dir)
}
