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
    /// SHA-256 of the JSON representation (field declaration order).
    ///
    /// Deterministic for a given struct definition. Reordering fields is a
    /// breaking change — existing hashes would no longer match.
    pub fn content_hash(&self) -> String {
        let (hash, _) = self.content_hash_with_json();
        hash
    }

    /// Returns `(sha256_hex, json_string)` to avoid double-serializing.
    pub(crate) fn content_hash_with_json(&self) -> (String, String) {
        let json = serde_json::to_string(self).expect("GameConfigRecord is always serializable");
        let hash = Sha256::digest(json.as_bytes());
        (format!("{hash:x}"), json)
    }
}

#[derive(Debug, Clone)]
pub struct PlayerRecord {
    pub id: String,
    pub display_name: String,
    pub created_at: String,
    /// Stable bot identifier from `bot.toml`. NULL on rows created via
    /// `ensure_player`; populated via `register_player`.
    pub agent_id: Option<String>,
    pub version: Option<String>,
    pub command: Option<String>,
    /// Free-form planner/runner metadata as JSON. Opaque to the store.
    pub metadata_json: Option<String>,
}

/// Identity-bearing player insert. Use this for tournament participants;
/// `ensure_player(id, name)` remains for ad-hoc / back-compat callers.
#[derive(Debug, Clone)]
pub struct NewPlayer {
    pub id: String,
    pub display_name: String,
    pub agent_id: Option<String>,
    pub version: Option<String>,
    pub command: Option<String>,
    pub metadata_json: Option<String>,
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

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct TournamentId(pub i64);

#[derive(Debug, Clone)]
pub struct TournamentRecord {
    pub id: TournamentId,
    pub format: String,
    pub target_games_per_matchup: Option<u32>,
    /// Opaque planner-defined config. The store does not validate this field.
    pub params_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewTournament {
    pub format: String,
    pub target_games_per_matchup: Option<u32>,
    pub params_json: String,
}

#[derive(Debug, Clone)]
pub struct TournamentParticipant {
    pub tournament_id: TournamentId,
    pub player_id: String,
    pub slot: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttemptStatus {
    Success,
    Failure,
}

impl AttemptStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            AttemptStatus::Success => "success",
            AttemptStatus::Failure => "failure",
        }
    }

    pub(crate) fn from_str(s: &str) -> Option<Self> {
        match s {
            "success" => Some(AttemptStatus::Success),
            "failure" => Some(AttemptStatus::Failure),
            _ => None,
        }
    }
}

/// Common identifying fields shared by both attempt variants.
#[derive(Debug, Clone)]
pub struct AttemptKey {
    pub tournament_id: TournamentId,
    pub game_config_id: String,
    pub player1_id: String,
    pub player2_id: String,
    pub seed: u64,
    pub repetition_index: u32,
    /// Per-matchup-key retry counter chosen by the session (next free integer).
    pub attempt_index: u32,
}

/// Input for `record_attempt`. The `outcome` variant is the type-level
/// guarantee that scores/turns are always set on success and never on failure;
/// the DB CHECK constraint mirrors this as defense in depth.
#[derive(Debug, Clone)]
pub struct NewAttempt {
    pub key: AttemptKey,
    /// Caller-supplied terminal timestamp. SQLite datetime string format
    /// (e.g. `"2026-05-06 12:34:56"`).
    pub finished_at: String,
    pub outcome: NewAttemptOutcome,
}

#[derive(Debug, Clone)]
pub enum NewAttemptOutcome {
    Success {
        player1_score: f64,
        player2_score: f64,
        turns: u32,
        /// SQLite datetime string (`datetime('now')` format).
        started_at: String,
    },
    Failure {
        failure_reason: String,
        /// `None` for spawn-failures (the bot never started). `Some` for
        /// post-start failures (timeout, crash, etc.).
        started_at: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct AttemptRecord {
    pub id: i64,
    pub key: AttemptKey,
    pub finished_at: String,
    pub outcome: AttemptOutcome,
}

/// Read-side mirror of [`NewAttemptOutcome`]. Variant-typed reads remove
/// the `Option<f64>` soup that the previous flat struct carried, and the
/// `match_attempts` CHECK constraint guarantees the variant fields are
/// non-NULL on success and NULL on failure.
#[derive(Debug, Clone)]
pub enum AttemptOutcome {
    Success {
        player1_score: f64,
        player2_score: f64,
        turns: u32,
        started_at: String,
    },
    Failure {
        failure_reason: String,
        /// `None` for spawn-failures (the bot never started). `Some` for
        /// post-start failures (timeout, crash, etc.).
        started_at: Option<String>,
    },
}

impl AttemptRecord {
    /// Convenience accessor over `outcome`. Lets callers filter / count by
    /// status without destructuring the enum.
    pub fn status(&self) -> AttemptStatus {
        match self.outcome {
            AttemptOutcome::Success { .. } => AttemptStatus::Success,
            AttemptOutcome::Failure { .. } => AttemptStatus::Failure,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Tournament-context player insert errors.
#[derive(Debug, thiserror::Error)]
pub enum RegisterPlayerError {
    #[error(transparent)]
    Db(#[from] EvalError),

    /// A row with this `id` exists but has different non-NULL identity fields.
    /// The user must either bump the player id or delete the existing row.
    #[error("player {id} already exists with conflicting identity fields: {fields:?}")]
    IdentityConflict { id: String, fields: Vec<String> },
}

#[derive(Debug, thiserror::Error)]
pub enum DeletePlayerError {
    #[error(transparent)]
    Db(#[from] EvalError),

    /// The player is referenced by tournament rows. Delete the listed
    /// tournaments first, or bump the player id.
    #[error("player is referenced by tournament history (tournaments: {tournament_ids:?})")]
    InTournamentHistory { tournament_ids: Vec<TournamentId> },
}

#[derive(Debug, thiserror::Error)]
pub enum AddTournamentPlayerError {
    #[error(transparent)]
    Db(#[from] EvalError),

    /// The player is already a participant in this tournament.
    #[error("player {player_id} already in tournament {tournament_id:?}")]
    PlayerAlreadyInTournament {
        tournament_id: TournamentId,
        player_id: String,
    },

    /// Slot is taken by a different player in this tournament.
    #[error("slot {slot} already occupied in tournament {tournament_id:?}")]
    SlotTaken {
        tournament_id: TournamentId,
        slot: i64,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RecordAttemptError {
    #[error(transparent)]
    Db(#[from] EvalError),

    /// SQLite stores INTEGER as signed i64. Planner-derived seeds must be
    /// masked to fit; this is a defense-in-depth check at the store boundary.
    #[error("seed {value} exceeds i64::MAX (cannot store as SQLite INTEGER)")]
    SeedOutOfRange { value: u64 },

    /// An attempt with this `(tournament, game_config, p1, p2,
    /// repetition_index, attempt_index)` already exists. Typically signals a
    /// planner bug (wrong `attempt_index`) or a resume race; the caller can
    /// pick the next free index from the in-memory matchup history and retry.
    #[error("attempt already exists for this matchup key")]
    AttemptAlreadyExists { key: AttemptKey },
}

impl From<rusqlite::Error> for RegisterPlayerError {
    fn from(e: rusqlite::Error) -> Self {
        RegisterPlayerError::Db(EvalError::Db(e))
    }
}

impl From<rusqlite::Error> for DeletePlayerError {
    fn from(e: rusqlite::Error) -> Self {
        DeletePlayerError::Db(EvalError::Db(e))
    }
}

impl From<rusqlite::Error> for AddTournamentPlayerError {
    fn from(e: rusqlite::Error) -> Self {
        AddTournamentPlayerError::Db(EvalError::Db(e))
    }
}

impl From<rusqlite::Error> for RecordAttemptError {
    fn from(e: rusqlite::Error) -> Self {
        RecordAttemptError::Db(EvalError::Db(e))
    }
}
