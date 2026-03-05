use serde::{Deserialize, Serialize};
use specta::Type;

use crate::json_store;

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct BotConfigEntry {
    pub id: String,
    pub name: String,
    pub command: String,
    pub working_dir: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn load_bot_configs(app: tauri::AppHandle) -> Result<Vec<BotConfigEntry>, String> {
    json_store::load(&app, "bots.json", Vec::new).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_bot_configs(
    app: tauri::AppHandle,
    configs: Vec<BotConfigEntry>,
) -> Result<(), String> {
    json_store::save(&app, "bots.json", &configs).await
}
