use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::Manager;

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct BotConfigEntry {
    pub id: String,
    pub name: String,
    pub command: String,
    pub working_dir: Option<String>,
}

fn bots_json_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
    Ok(dir.join("bots.json"))
}

#[tauri::command]
#[specta::specta]
pub async fn load_bot_configs(app: tauri::AppHandle) -> Result<Vec<BotConfigEntry>, String> {
    let path = bots_json_path(&app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(&path).map_err(|e| format!("read error: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("parse error: {e}"))
}

#[tauri::command]
#[specta::specta]
pub async fn save_bot_configs(
    app: tauri::AppHandle,
    configs: Vec<BotConfigEntry>,
) -> Result<(), String> {
    let path = bots_json_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir error: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&configs).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write error: {e}"))
}
