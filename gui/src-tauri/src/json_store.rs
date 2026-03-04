use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::Serialize;
use tauri::Manager;

/// Resolve a JSON file path under the app data directory.
fn json_path(app: &tauri::AppHandle, filename: &str) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
    Ok(dir.join(filename))
}

/// Load a JSON file from the app data directory, returning `default` if missing.
pub async fn load<T: DeserializeOwned>(
    app: &tauri::AppHandle,
    filename: &str,
    default: impl FnOnce() -> T,
) -> Result<T, String> {
    let path = json_path(app, filename)?;
    match tokio::fs::read_to_string(&path).await {
        Ok(data) => serde_json::from_str(&data).map_err(|e| format!("parse error: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(default()),
        Err(e) => Err(format!("read error: {e}")),
    }
}

/// Save a value as pretty JSON to the app data directory.
pub async fn save<T: Serialize>(
    app: &tauri::AppHandle,
    filename: &str,
    data: &T,
) -> Result<(), String> {
    let path = json_path(app, filename)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("mkdir error: {e}"))?;
    }
    let json = serde_json::to_string_pretty(data).map_err(|e| format!("serialize: {e}"))?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|e| format!("write error: {e}"))
}
