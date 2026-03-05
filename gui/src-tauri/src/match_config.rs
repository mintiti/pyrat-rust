use crate::commands::MatchConfigParams;
use crate::json_store;

#[tauri::command]
#[specta::specta]
pub async fn load_match_config(app: tauri::AppHandle) -> Result<MatchConfigParams, String> {
    json_store::load(&app, "match_config.json", MatchConfigParams::default).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_match_config(
    app: tauri::AppHandle,
    config: MatchConfigParams,
) -> Result<(), String> {
    json_store::save(&app, "match_config.json", &config).await
}
