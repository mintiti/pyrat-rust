use std::path::PathBuf;

use tauri::Manager;

use crate::commands::MatchConfigParams;

fn config_json_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
    Ok(dir.join("match_config.json"))
}

#[tauri::command]
#[specta::specta]
pub async fn load_match_config(app: tauri::AppHandle) -> Result<MatchConfigParams, String> {
    let path = config_json_path(&app)?;
    if !path.exists() {
        return Ok(MatchConfigParams::default());
    }
    let data = std::fs::read_to_string(&path).map_err(|e| format!("read error: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("parse error: {e}"))
}

#[tauri::command]
#[specta::specta]
pub async fn save_match_config(
    app: tauri::AppHandle,
    config: MatchConfigParams,
) -> Result<(), String> {
    let path = config_json_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir error: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&config).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write error: {e}"))
}
