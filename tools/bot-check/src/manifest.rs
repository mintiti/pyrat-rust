use std::path::Path;

use serde::Deserialize;

/// Parsed `bot.toml` manifest.
#[derive(Debug, Deserialize)]
pub struct BotManifest {
    pub settings: Settings,
    #[serde(default)]
    #[allow(dead_code)]
    pub details: Details,
}

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub name: String,
    pub agent_id: String,
    pub run_command: String,
}

#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)] // Fields are part of the manifest schema, parsed but not used by check flow.
pub struct Details {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub developer: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug)]
pub enum ManifestError {
    NotFound(std::path::PathBuf),
    Read(std::io::Error),
    Parse(toml::de::Error),
    Validation(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "bot.toml not found at {}", path.display()),
            Self::Read(e) => write!(f, "failed to read bot.toml: {e}"),
            Self::Parse(e) => write!(f, "failed to parse bot.toml: {e}"),
            Self::Validation(msg) => write!(f, "invalid bot.toml: {msg}"),
        }
    }
}

impl BotManifest {
    /// Load and validate a bot manifest from a directory.
    pub fn load(bot_dir: &Path) -> Result<Self, ManifestError> {
        let path = bot_dir.join("bot.toml");
        if !path.exists() {
            return Err(ManifestError::NotFound(path));
        }

        let contents = std::fs::read_to_string(&path).map_err(ManifestError::Read)?;
        let manifest: Self = toml::from_str(&contents).map_err(ManifestError::Parse)?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if self.settings.name.trim().is_empty() {
            return Err(ManifestError::Validation("settings.name is empty".into()));
        }
        if self.settings.agent_id.trim().is_empty() {
            return Err(ManifestError::Validation(
                "settings.agent_id is empty".into(),
            ));
        }
        if self.settings.run_command.trim().is_empty() {
            return Err(ManifestError::Validation(
                "settings.run_command is empty".into(),
            ));
        }
        Ok(())
    }
}
