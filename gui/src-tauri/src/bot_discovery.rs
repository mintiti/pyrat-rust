use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;
use tracing::warn;
use walkdir::WalkDir;

use crate::json_store;

// ---------------------------------------------------------------------------
// Public types (exported to frontend via specta)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct DiscoveredBot {
    pub agent_id: String,
    pub name: String,
    pub run_command: String,
    /// Absolute path to the directory containing bot.toml.
    pub working_dir: String,
    pub description: String,
    pub developer: String,
    pub language: String,
    pub tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Internal parsing types (not specta-exported)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BotManifest {
    settings: Settings,
    #[serde(default)]
    details: Details,
}

#[derive(Debug, Deserialize)]
struct Settings {
    name: String,
    agent_id: String,
    run_command: String,
}

#[derive(Debug, Default, Deserialize)]
struct Details {
    #[serde(default)]
    description: String,
    #[serde(default)]
    developer: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Directories to skip during traversal
// ---------------------------------------------------------------------------

const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "venv",
    "__pycache__",
    ".git",
    ".venv",
];

fn should_skip(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name)
}

// ---------------------------------------------------------------------------
// Discovery logic
// ---------------------------------------------------------------------------

fn parse_bot_toml(path: &Path) -> Option<DiscoveredBot> {
    let contents = std::fs::read_to_string(path).ok()?;
    let manifest: BotManifest = match toml::from_str(&contents) {
        Ok(m) => m,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to parse bot.toml");
            return None;
        },
    };

    let s = &manifest.settings;
    if s.name.trim().is_empty() || s.agent_id.trim().is_empty() || s.run_command.trim().is_empty() {
        warn!(path = %path.display(), "bot.toml has empty required fields");
        return None;
    }

    let working_dir = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    Some(DiscoveredBot {
        agent_id: s.agent_id.clone(),
        name: s.name.clone(),
        run_command: s.run_command.clone(),
        working_dir,
        description: manifest.details.description,
        developer: manifest.details.developer,
        language: manifest.details.language,
        tags: manifest.details.tags,
    })
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn load_scan_paths(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    json_store::load(&app, "scan_paths.json", Vec::new).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_scan_paths(app: tauri::AppHandle, paths: Vec<String>) -> Result<(), String> {
    json_store::save(&app, "scan_paths.json", &paths).await
}

#[tauri::command]
#[specta::specta]
pub fn discover_bots(paths: Vec<String>) -> Vec<DiscoveredBot> {
    let mut seen = HashSet::new();
    let mut bots = Vec::new();

    for base in &paths {
        let walker = WalkDir::new(base)
            .max_depth(3)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    e.file_name()
                        .to_str()
                        .map(|n| !should_skip(n))
                        .unwrap_or(true)
                } else {
                    true
                }
            });

        for entry in walker.flatten() {
            if entry.file_type().is_file() && entry.file_name() == "bot.toml" {
                if let Some(bot) = parse_bot_toml(entry.path()) {
                    if seen.insert(bot.agent_id.clone()) {
                        bots.push(bot);
                    }
                }
            }
        }
    }

    bots
}
