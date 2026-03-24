use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameConfigRecord {
    pub width: u32,
    pub height: u32,
    pub max_turns: u32,
    pub wall_density: f64,
    pub mud_density: f64,
    pub mud_range: u32,
    pub connected: bool,
    pub symmetric: bool,
    pub cheese_count: u32,
    pub cheese_symmetric: bool,
}

impl GameConfigRecord {
    /// SHA-256 of the canonical (sorted-key) JSON representation.
    pub fn content_hash(&self) -> String {
        let json = serde_json::to_string(self).expect("GameConfigRecord is always serializable");
        let hash = Sha256::digest(json.as_bytes());
        format!("{hash:x}")
    }
}

#[derive(Debug, Clone)]
pub struct PlayerRecord {
    pub id: String,
    pub display_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct GameResultRecord {
    pub id: i64,
    pub game_config_id: String,
    pub player1_id: String,
    pub player2_id: String,
    pub player1_score: f64,
    pub player2_score: f64,
    pub turns: u32,
    pub played_at: String,
}

/// Input for recording a new game result. Avoids a wall of positional args.
pub struct NewGameResult {
    pub game_config_id: String,
    pub player1_id: String,
    pub player2_id: String,
    pub player1_score: f64,
    pub player2_score: f64,
    pub turns: u32,
}

/// Optional filters for querying results.
#[derive(Default)]
pub struct ResultFilter {
    pub player_id: Option<String>,
    pub game_config_id: Option<String>,
    pub after: Option<String>,
    pub before: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
