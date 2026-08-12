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
    /// Player start-position strategy. Skipped on serialization for `Corners`
    /// (the default), so corner configs — every pre-existing row and the
    /// committed ladder — keep their exact JSON and `content_hash`. Only
    /// `Random` configs carry the field and get a distinct hash.
    #[serde(default, skip_serializing_if = "PlayerStartRecord::is_corners")]
    pub player_start: PlayerStartRecord,
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

/// Player start-position strategy recorded with a game config. Store-native
/// (no engine dependency — same boundary discipline as [`SeatOrientation`]);
/// the engine's `PlayerStrategy` is mapped to this at the session boundary.
///
/// `Corners` is the default and is omitted on serialization (see
/// [`GameConfigRecord::player_start`]) so corner-start configs hash exactly as
/// they did before this field existed. Only `Random` configs carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlayerStartRecord {
    /// P1 at (0,0), P2 at the opposite corner — the historical sole behaviour
    /// and the only start strategy the committed ladder uses.
    #[default]
    Corners,
    /// Both players placed at random (seeded), not necessarily mutual mirrors.
    Random,
}

impl PlayerStartRecord {
    /// `skip_serializing_if` predicate: omit the field for corner starts so
    /// their JSON (and `content_hash`) is byte-identical to the pre-field
    /// encoding. Kept in sync with `#[default]` above.
    #[allow(clippy::trivially_copy_pass_by_ref)] // serde requires `&self`
    pub(crate) fn is_corners(&self) -> bool {
        matches!(self, PlayerStartRecord::Corners)
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

/// Which engine seat (Rat = slot 0) the lex-min player occupied in a game.
///
/// An informational marker stored alongside the canonical row. Scores and
/// `player1_id`/`player2_id` are always written in canonical (lex-min,
/// lex-max) order, so every read-side path (Elo, head-to-head, resume) is
/// seat-agnostic. `orientation` only records *who was Rat*, for auditing and
/// replay-correct rendering. Not part of the matchup UNIQUE constraint — two
/// seatings of the same maze are distinguished by their `repetition_index`
/// (the chess-paired `2k` / `2k+1` slots), not by this column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SeatOrientation {
    /// Lex-min player was Rat (slot 0). The historical default: every
    /// pre-`MIGRATION_5` row was played this way, so it maps to DB value 0.
    #[default]
    Canonical,
    /// Lex-max player was Rat (slot 0): the flipped seating of a chess pair.
    Flipped,
}

impl SeatOrientation {
    /// DB integer encoding. `0 = Canonical` is also the `MIGRATION_5` column
    /// default, so existing rows decode correctly.
    pub fn to_db(self) -> i64 {
        match self {
            SeatOrientation::Canonical => 0,
            SeatOrientation::Flipped => 1,
        }
    }

    /// Decode a DB integer. `None` for any value the schema cannot produce —
    /// the single conversion site (`read_attempt_row`) turns this into a
    /// typed read error rather than a silent mis-seat.
    pub fn from_db(v: i64) -> Option<Self> {
        match v {
            0 => Some(SeatOrientation::Canonical),
            1 => Some(SeatOrientation::Flipped),
            _ => None,
        }
    }

    /// Map seat-order values (slot 0, slot 1) to canonical order (lex-min,
    /// lex-max). The single source of truth for the seat→canonical swap,
    /// shared by every write/emit site so they cannot drift. Generic so the
    /// same helper serves `f64` store scores and `f32` live-stream scores.
    pub fn canonicalize<T>(self, slot0: T, slot1: T) -> (T, T) {
        match self {
            SeatOrientation::Canonical => (slot0, slot1),
            SeatOrientation::Flipped => (slot1, slot0),
        }
    }
}

/// Timing mode used for every match in a tournament.
///
/// Store-native mirror of the wire protocol enum: the eval store deliberately
/// does not depend on host/wire crates. Keeping the mode in the durable
/// methodology means a future Clock-mode tournament will not be
/// indistinguishable from today's Wait-mode runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TournamentTimingMode {
    Wait,
    Clock,
}

impl TournamentTimingMode {
    pub(crate) fn to_db(self) -> i64 {
        match self {
            Self::Wait => 0,
            Self::Clock => 1,
        }
    }

    pub(crate) fn from_db(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Wait),
            1 => Some(Self::Clock),
            _ => None,
        }
    }
}

/// Execution conditions needed to interpret one tournament's results.
///
/// This is separate from opaque planner params and the content-addressed game
/// config: all of these values can change whether a match succeeds, times out,
/// or competes for host resources. Tournament rows written before migration 7
/// expose `None` for the whole methodology rather than inheriting today's
/// defaults and pretending those were the conditions that actually ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TournamentMethodology {
    pub timing_mode: TournamentTimingMode,
    pub move_timeout_ms: u32,
    pub preprocessing_timeout_ms: u32,
    pub startup_timeout_ms: u32,
    pub configure_timeout_ms: u32,
    pub network_grace_ms: u32,
    pub max_parallel: u32,
}

/// Durable lifecycle of one tournament row. `LegacyUnknown` is an honest
/// migration state, not an inferred partial run: pre-migration attempts do
/// not reveal whether the process completed, was stopped, or crashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TournamentLifecycle {
    LegacyUnknown,
    Preparing,
    Running,
    Completed,
    Stopped,
    Failed,
}

impl TournamentLifecycle {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::LegacyUnknown => "legacy_unknown",
            Self::Preparing => "preparing",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "legacy_unknown" => Some(Self::LegacyUnknown),
            "preparing" => Some(Self::Preparing),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "stopped" => Some(Self::Stopped),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Why a terminal tournament stopped changing. Completion quality is kept
/// separate from rating readiness: a schedule may finish with exhausted
/// slots and may still be statistically unusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TournamentTerminalKind {
    Completed,
    CompletedWithFailures,
    UserStopped,
    InfrastructureFailure,
}

impl TournamentTerminalKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::CompletedWithFailures => "completed_with_failures",
            Self::UserStopped => "user_stopped",
            Self::InfrastructureFailure => "infrastructure_failure",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "completed" => Some(Self::Completed),
            "completed_with_failures" => Some(Self::CompletedWithFailures),
            "user_stopped" => Some(Self::UserStopped),
            "infrastructure_failure" => Some(Self::InfrastructureFailure),
            _ => None,
        }
    }
}

/// Whether the stored evidence can support the rating surface. This is
/// orthogonal to lifecycle and schedule completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TournamentRatingStatus {
    LegacyUnknown,
    Provisional,
    Rateable,
    InsufficientGames,
    DisconnectedGraph,
    EstimatorFailed,
}

impl TournamentRatingStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::LegacyUnknown => "legacy_unknown",
            Self::Provisional => "provisional",
            Self::Rateable => "rateable",
            Self::InsufficientGames => "insufficient_games",
            Self::DisconnectedGraph => "disconnected_graph",
            Self::EstimatorFailed => "estimator_failed",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "legacy_unknown" => Some(Self::LegacyUnknown),
            "provisional" => Some(Self::Provisional),
            "rateable" => Some(Self::Rateable),
            "insufficient_games" => Some(Self::InsufficientGames),
            "disconnected_graph" => Some(Self::DisconnectedGraph),
            "estimator_failed" => Some(Self::EstimatorFailed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentTerminal {
    pub kind: TournamentTerminalKind,
    /// Exact human-actionable detail. Completion without failures normally
    /// has no reason; stopped/failed rows always carry one.
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TournamentRecord {
    pub id: TournamentId,
    /// Optional human-readable name (e.g. "ckpt-1200"). NULL for rows created
    /// without one, such as CLI tournaments — they identify by id + created_at.
    pub name: Option<String>,
    pub format: String,
    pub target_games_per_matchup: Option<u32>,
    /// Opaque planner-defined config. The store does not validate this field.
    pub params_json: String,
    /// Content-hashed id of the `game_configs` row this tournament uses.
    /// Validated at insert time and on resume.
    pub game_config_id: String,
    /// Seed fed into the planner's `matchup_seed` derivation. Bounded at
    /// insert time to `<= i64::MAX` (see `NewTournament.tournament_seed`),
    /// so this value round-trips bit-identically with what the caller
    /// passed.
    pub tournament_seed: u64,
    /// Exact execution conditions for current rows. `None` means the row
    /// predates durable methodology; callers must present that as unknown.
    pub methodology: Option<TournamentMethodology>,
    pub lifecycle: TournamentLifecycle,
    pub started_at: Option<String>,
    pub terminal_at: Option<String>,
    pub terminal: Option<TournamentTerminal>,
    pub rating_status: TournamentRatingStatus,
    pub rating_reason: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewTournament {
    /// Optional human-readable name. `None` leaves the column NULL (CLI path);
    /// the GUI supplies one so stored tournaments are distinguishable.
    pub name: Option<String>,
    pub format: String,
    pub target_games_per_matchup: Option<u32>,
    pub params_json: String,
    /// Must reference an existing `game_configs.id`. The store validates this
    /// up front and returns `CreateTournamentError::GameConfigNotFound` for a
    /// missing config.
    pub game_config_id: String,
    /// Tournament-level seed for `matchup_seed`. Must be `<= i64::MAX`:
    /// SQLite's INTEGER column is signed, and a high-bit seed would not
    /// round-trip without silent truncation. `create_tournament` rejects
    /// out-of-range seeds with `CreateTournamentError::SeedOutOfRange`
    /// rather than masking, so the value the caller passes always equals
    /// the value the row stores.
    pub tournament_seed: u64,
    /// Exact execution conditions. `None` is retained only for compatibility
    /// callers and legacy fixtures that genuinely do not know them.
    pub methodology: Option<TournamentMethodology>,
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

/// Stable typed category for a failed attempt. Store-native so the durable
/// schema does not depend on orchestrator/host crates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptFailureKind {
    Timeout,
    Disconnected,
    SpawnFailed,
    HandshakeTimeout,
    ProtocolError,
    Cancelled,
    Infrastructure,
    Other,
}

impl AttemptFailureKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Disconnected => "disconnected",
            Self::SpawnFailed => "spawn_failed",
            Self::HandshakeTimeout => "handshake_timeout",
            Self::ProtocolError => "protocol_error",
            Self::Cancelled => "cancelled",
            Self::Infrastructure => "infrastructure",
            Self::Other => "other",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "timeout" => Some(Self::Timeout),
            "disconnected" => Some(Self::Disconnected),
            "spawn_failed" => Some(Self::SpawnFailed),
            "handshake_timeout" => Some(Self::HandshakeTimeout),
            "protocol_error" => Some(Self::ProtocolError),
            "cancelled" => Some(Self::Cancelled),
            "infrastructure" => Some(Self::Infrastructure),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptFailurePhase {
    Setup,
    Preprocessing,
    Sync,
    Move,
}

impl AttemptFailurePhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Preprocessing => "preprocessing",
            Self::Sync => "sync",
            Self::Move => "move",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "setup" => Some(Self::Setup),
            "preprocessing" => Some(Self::Preprocessing),
            "sync" => Some(Self::Sync),
            "move" => Some(Self::Move),
            _ => None,
        }
    }
}

/// Canonical durable failure report. Slot/repetition/seat live on
/// [`AttemptKey`]; this carries classification, attribution, and exact text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptFailureReport {
    pub kind: AttemptFailureKind,
    pub phase: Option<AttemptFailurePhase>,
    pub failing_player_id: Option<String>,
    pub message: String,
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
///
/// `match_id`, `seed`, and `orientation` are carried here but are NOT part of
/// the matchup UNIQUE constraint — they are forensic/informational fields
/// that ride alongside the identity tuple. `match_id` is nullable for rows
/// written before migration 6; current sessions persist it so durable attempt
/// rows can be joined back to replay files and live UIs can reconcile missed
/// lifecycle events from the store.
#[derive(Debug, Clone)]
pub struct AttemptKey {
    pub tournament_id: TournamentId,
    pub game_config_id: String,
    pub player1_id: String,
    pub player2_id: String,
    pub match_id: Option<u64>,
    pub seed: u64,
    pub repetition_index: u32,
    /// Per-matchup-key retry counter chosen by the session (next free integer).
    pub attempt_index: u32,
    /// Which player was Rat (slot 0) in this game. Informational: scores are
    /// stored canonical regardless. Defaults to `Canonical` for legacy rows.
    pub orientation: SeatOrientation,
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
        report: AttemptFailureReport,
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
        /// `None` only for rows written before migration 8. Current writers
        /// always persist this report; compatibility readers may parse the
        /// legacy reason string as a last resort.
        report: Option<AttemptFailureReport>,
        /// `None` for spawn-failures (the bot never started). `Some` for
        /// post-start failures (timeout, crash, etc.).
        started_at: Option<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// A migration refused to run because pre-flight state was unsafe to
    /// upgrade automatically. The operator must manually backfill or wipe.
    #[error("migration {version} blocked: {message}")]
    MigrationBlocked { version: u32, message: String },
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
pub enum CreateTournamentError {
    #[error(transparent)]
    Db(#[from] EvalError),

    /// `NewTournament.game_config_id` does not reference an existing row in
    /// `game_configs`. The caller must `ensure_game_config(...)` first.
    #[error("game_config_id {0:?} does not exist in game_configs")]
    GameConfigNotFound(String),

    /// `NewTournament.tournament_seed` exceeds `i64::MAX`. SQLite's INTEGER
    /// is signed, so values with the high bit set cannot round-trip
    /// without truncation. Reject at the boundary instead of silently
    /// masking so the caller's seed and the stored seed always agree.
    #[error("tournament_seed {seed} exceeds i64::MAX; SQLite INTEGER is signed")]
    SeedOutOfRange { seed: u64 },
}

impl From<rusqlite::Error> for CreateTournamentError {
    fn from(e: rusqlite::Error) -> Self {
        CreateTournamentError::Db(EvalError::Db(e))
    }
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

    #[error("match id {value} exceeds i64::MAX (cannot store as SQLite INTEGER)")]
    MatchIdOutOfRange { value: u64 },

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

#[cfg(test)]
mod tests {
    use super::*;

    fn corners_record() -> GameConfigRecord {
        GameConfigRecord {
            width: 21,
            height: 15,
            max_turns: 300,
            wall_density: 0.7,
            mud_density: 0.1,
            mud_range: 3,
            connected: true,
            symmetric: true,
            cheese_count: 41,
            cheese_symmetric: true,
            player_start: PlayerStartRecord::Corners,
        }
    }

    /// Adding `player_start` must not change the JSON (or `content_hash`) of a
    /// corner-start config: every pre-existing row and the committed ladder key
    /// on that hash. The frozen `Legacy` shape is the exact pre-field encoding.
    #[test]
    fn corners_config_hash_byte_identical_to_pre_field_encoding() {
        #[derive(Serialize)]
        struct Legacy {
            width: u32,
            height: u32,
            max_turns: u32,
            wall_density: f64,
            mud_density: f64,
            mud_range: u32,
            connected: bool,
            symmetric: bool,
            cheese_count: u32,
            cheese_symmetric: bool,
        }
        let legacy = Legacy {
            width: 21,
            height: 15,
            max_turns: 300,
            wall_density: 0.7,
            mud_density: 0.1,
            mud_range: 3,
            connected: true,
            symmetric: true,
            cheese_count: 41,
            cheese_symmetric: true,
        };
        let legacy_json = serde_json::to_string(&legacy).unwrap();
        let (hash, json) = corners_record().content_hash_with_json();

        assert!(
            !json.contains("player_start"),
            "corner config must omit the field: {json}"
        );
        assert_eq!(json, legacy_json, "JSON must match the pre-field encoding");
        let legacy_hash = format!("{:x}", Sha256::digest(legacy_json.as_bytes()));
        assert_eq!(hash, legacy_hash, "content_hash must be unchanged");
    }

    /// A random-start config carries the field, so it hashes distinctly from an
    /// otherwise-identical corner config.
    #[test]
    fn random_start_config_serializes_field_and_changes_hash() {
        let corners = corners_record();
        let random = GameConfigRecord {
            player_start: PlayerStartRecord::Random,
            ..corners.clone()
        };
        let (random_hash, random_json) = random.content_hash_with_json();

        assert!(
            random_json.contains(r#""player_start":"Random""#),
            "random config must carry the field: {random_json}"
        );
        assert_ne!(corners.content_hash(), random_hash);
    }
}
