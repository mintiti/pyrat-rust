use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::elo::HeadToHead;
use crate::schema;
use crate::types::{
    AddTournamentPlayerError, AttemptKey, AttemptOutcome, AttemptRecord, AttemptStatus,
    DeletePlayerError, EvalError, GameConfigRecord, GameResultRecord, NewAttempt,
    NewAttemptOutcome, NewGameResult, NewPlayer, NewTournament, PlayerRecord, RecordAttemptError,
    RegisterPlayerError, ResultFilter, TournamentId, TournamentParticipant, TournamentRecord,
};

/// SQLite-backed store for game results, players, tournaments, and match
/// attempts.
///
/// Holds a single `rusqlite::Connection`. `Connection: !Sync` is enforced by
/// the compiler, so `Arc<EvalStore>` shared across threads will not compile.
/// Concurrent consumers (e.g. the orchestrator running matches in parallel)
/// must wrap in `Arc<Mutex<_>>` or open a fresh connection per thread.
pub struct EvalStore {
    conn: Connection,
}

impl EvalStore {
    fn from_connection(mut conn: Connection) -> Result<Self, EvalError> {
        schema::initialize(&mut conn)?;
        Ok(Self { conn })
    }

    /// Open (or create) a store at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EvalError> {
        Self::from_connection(Connection::open(path)?)
    }

    /// In-memory store for tests.
    pub fn open_in_memory() -> Result<Self, EvalError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// Insert a player if it doesn't already exist. Back-compat path; does not
    /// populate the identity columns. Tournament use should call
    /// [`EvalStore::register_player`] instead.
    pub fn ensure_player(&self, id: &str, display_name: &str) -> Result<(), EvalError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO players (id, display_name) VALUES (?1, ?2)",
            params![id, display_name],
        )?;
        Ok(())
    }

    /// Insert-or-error-on-conflict for tournament-context players. A player
    /// is a specific rated *version* of a bot — silent re-pointing of an id
    /// would retroactively rewrite tournament identity, so conflicts surface.
    ///
    /// Behavior:
    /// - No row with `id`: insert with all provided fields.
    /// - Row exists with NULL identity columns: fill them in (back-compat for
    ///   rows created via `ensure_player`).
    /// - Row exists with conflicting non-NULL identity column: return
    ///   [`RegisterPlayerError::IdentityConflict`] listing the conflicting
    ///   field names.
    /// - Row exists with identical identity: no-op success.
    pub fn register_player(&self, p: &NewPlayer) -> Result<(), RegisterPlayerError> {
        let existing = self
            .conn
            .query_row(
                "SELECT agent_id, version, command, metadata_json
                 FROM players WHERE id = ?1",
                params![p.id],
                |row| {
                    Ok(ExistingIdentity {
                        agent_id: row.get(0)?,
                        version: row.get(1)?,
                        command: row.get(2)?,
                        metadata_json: row.get(3)?,
                    })
                },
            )
            .optional()?;

        match existing {
            None => {
                self.conn.execute(
                    "INSERT INTO players (id, display_name, agent_id, version, command, metadata_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![p.id, p.display_name, p.agent_id, p.version, p.command, p.metadata_json],
                )?;
            },
            Some(ex) => {
                let mut conflicts = Vec::new();
                check_conflict(&mut conflicts, "agent_id", &ex.agent_id, &p.agent_id);
                check_conflict(&mut conflicts, "version", &ex.version, &p.version);
                check_conflict(&mut conflicts, "command", &ex.command, &p.command);
                check_conflict(
                    &mut conflicts,
                    "metadata_json",
                    &ex.metadata_json,
                    &p.metadata_json,
                );
                if !conflicts.is_empty() {
                    return Err(RegisterPlayerError::IdentityConflict {
                        id: p.id.clone(),
                        fields: conflicts,
                    });
                }
                self.conn.execute(
                    "UPDATE players
                       SET agent_id      = COALESCE(agent_id, ?1),
                           version       = COALESCE(version, ?2),
                           command       = COALESCE(command, ?3),
                           metadata_json = COALESCE(metadata_json, ?4)
                     WHERE id = ?5",
                    params![p.agent_id, p.version, p.command, p.metadata_json, p.id],
                )?;
            },
        }
        Ok(())
    }

    /// Insert a game config (keyed by content hash) if it doesn't already exist.
    /// Returns the content hash used as the ID.
    pub fn ensure_game_config(&self, config: &GameConfigRecord) -> Result<String, EvalError> {
        let (id, json) = config.content_hash_with_json();
        self.conn.execute(
            "INSERT OR IGNORE INTO game_configs (id, config_json) VALUES (?1, ?2)",
            params![id, json],
        )?;
        Ok(id)
    }

    /// Append a game result. Returns the autoincrement row ID.
    pub fn record_result(&self, result: &NewGameResult) -> Result<i64, EvalError> {
        self.conn.execute(
            "INSERT INTO game_results (game_config_id, player1_id, player2_id, player1_score, player2_score, turns)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                result.game_config_id,
                result.player1_id,
                result.player2_id,
                result.player1_score,
                result.player2_score,
                result.turns,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Query results with optional filters.
    pub fn get_results(&self, filter: &ResultFilter) -> Result<Vec<GameResultRecord>, EvalError> {
        let mut clauses = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(pid) = &filter.player_id {
            clauses.push("(player1_id = ? OR player2_id = ?)");
            values.push(Box::new(pid.clone()));
            values.push(Box::new(pid.clone()));
        }
        if let Some(cid) = &filter.game_config_id {
            clauses.push("game_config_id = ?");
            values.push(Box::new(cid.clone()));
        }
        if let Some(after) = &filter.after {
            clauses.push("played_at >= ?");
            values.push(Box::new(after.clone()));
        }
        if let Some(before) = &filter.before {
            clauses.push("played_at <= ?");
            values.push(Box::new(before.clone()));
        }

        let where_clause = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };

        let sql = format!(
            "SELECT id, game_config_id, player1_id, player2_id, player1_score, player2_score, turns, played_at
             FROM game_results{where_clause} ORDER BY id"
        );

        let params_ref: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| &**v).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            Ok(GameResultRecord {
                id: row.get(0)?,
                game_config_id: row.get(1)?,
                player1_id: row.get(2)?,
                player2_id: row.get(3)?,
                player1_score: row.get(4)?,
                player2_score: row.get(5)?,
                turns: row.get(6)?,
                played_at: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(EvalError::from)
    }

    /// List all players.
    pub fn get_players(&self) -> Result<Vec<PlayerRecord>, EvalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, display_name, created_at, agent_id, version, command, metadata_json
               FROM players ORDER BY id",
        )?;
        let rows = stmt.query_map([], read_player_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(EvalError::from)
    }

    /// Single-row read by id. Used for resume paths that need identity back
    /// out without scanning the full pool.
    pub fn get_player(&self, id: &str) -> Result<Option<PlayerRecord>, EvalError> {
        self.conn
            .query_row(
                "SELECT id, display_name, created_at, agent_id, version, command, metadata_json
                   FROM players WHERE id = ?1",
                params![id],
                read_player_row,
            )
            .optional()
            .map_err(EvalError::from)
    }

    /// List all game configs with their IDs.
    pub fn get_game_configs(&self) -> Result<Vec<(String, GameConfigRecord)>, EvalError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, config_json FROM game_configs ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let json: String = row.get(1)?;
            Ok((id, json))
        })?;

        rows.collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(id, json)| {
                let config: GameConfigRecord = serde_json::from_str(&json)?;
                Ok((id, config))
            })
            .collect()
    }

    /// Delete a player and cascade to their `game_results`. Errors with
    /// [`DeletePlayerError::InTournamentHistory`] if the player is referenced
    /// by `tournament_players` or `match_attempts`.
    ///
    /// Returns `Ok(true)` if the player existed and was deleted, `Ok(false)`
    /// if no such player. The pre-check + delete sequence is on a single
    /// connection (rusqlite's `Connection: !Sync` rules out concurrent races).
    pub fn delete_player(&self, id: &str) -> Result<bool, DeletePlayerError> {
        let blocking = self.tournaments_referencing_player(id)?;
        if !blocking.is_empty() {
            return Err(DeletePlayerError::InTournamentHistory {
                tournament_ids: blocking,
            });
        }
        let deleted = self
            .conn
            .execute("DELETE FROM players WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }

    fn tournaments_referencing_player(&self, id: &str) -> Result<Vec<TournamentId>, EvalError> {
        let mut stmt = self.conn.prepare(
            "SELECT tournament_id FROM tournament_players WHERE player_id = ?1
             UNION
             SELECT tournament_id FROM match_attempts
                    WHERE player1_id = ?1 OR player2_id = ?1
             ORDER BY tournament_id",
        )?;
        let rows = stmt.query_map(params![id], |row| {
            let id: i64 = row.get(0)?;
            Ok(TournamentId(id))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(EvalError::from)
    }

    pub fn create_tournament(&self, t: &NewTournament) -> Result<TournamentId, EvalError> {
        self.conn.execute(
            "INSERT INTO tournaments (format, target_games_per_matchup, params_json)
             VALUES (?1, ?2, ?3)",
            params![t.format, t.target_games_per_matchup, t.params_json],
        )?;
        Ok(TournamentId(self.conn.last_insert_rowid()))
    }

    pub fn get_tournament(&self, id: TournamentId) -> Result<Option<TournamentRecord>, EvalError> {
        self.conn
            .query_row(
                "SELECT id, format, target_games_per_matchup, params_json, created_at
                   FROM tournaments WHERE id = ?1",
                params![id.0],
                read_tournament_row,
            )
            .optional()
            .map_err(EvalError::from)
    }

    pub fn list_tournaments(&self) -> Result<Vec<TournamentRecord>, EvalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, format, target_games_per_matchup, params_json, created_at
               FROM tournaments ORDER BY id",
        )?;
        let rows = stmt.query_map([], read_tournament_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(EvalError::from)
    }

    /// Delete a tournament. Cascades to `tournament_players` and
    /// `match_attempts` via FK. Players and `game_results` survive.
    /// Returns `true` if the tournament existed.
    pub fn delete_tournament(&self, id: TournamentId) -> Result<bool, EvalError> {
        let deleted = self
            .conn
            .execute("DELETE FROM tournaments WHERE id = ?1", params![id.0])?;
        Ok(deleted > 0)
    }

    /// Insert a tournament participant. Distinguishes `(tournament_id,
    /// player_id)` PK conflict from `(tournament_id, slot)` UNIQUE conflict
    /// via pre-check so callers get typed errors instead of a generic SQL
    /// constraint violation.
    pub fn add_tournament_player(
        &self,
        tournament_id: TournamentId,
        player_id: &str,
        slot: i64,
    ) -> Result<(), AddTournamentPlayerError> {
        let already_in: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tournament_players
                            WHERE tournament_id = ?1 AND player_id = ?2)",
            params![tournament_id.0, player_id],
            |row| row.get(0),
        )?;
        if already_in {
            return Err(AddTournamentPlayerError::PlayerAlreadyInTournament {
                tournament_id,
                player_id: player_id.to_string(),
            });
        }
        let slot_taken: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tournament_players
                            WHERE tournament_id = ?1 AND slot = ?2)",
            params![tournament_id.0, slot],
            |row| row.get(0),
        )?;
        if slot_taken {
            return Err(AddTournamentPlayerError::SlotTaken {
                tournament_id,
                slot,
            });
        }
        self.conn.execute(
            "INSERT INTO tournament_players (tournament_id, player_id, slot)
             VALUES (?1, ?2, ?3)",
            params![tournament_id.0, player_id, slot],
        )?;
        Ok(())
    }

    pub fn get_tournament_players(
        &self,
        tournament_id: TournamentId,
    ) -> Result<Vec<TournamentParticipant>, EvalError> {
        let mut stmt = self.conn.prepare(
            "SELECT tournament_id, player_id, slot
               FROM tournament_players WHERE tournament_id = ?1
              ORDER BY slot",
        )?;
        let rows = stmt.query_map(params![tournament_id.0], |row| {
            Ok(TournamentParticipant {
                tournament_id: TournamentId(row.get(0)?),
                player_id: row.get(1)?,
                slot: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(EvalError::from)
    }

    /// Insert a match attempt row. The [`NewAttempt`] enum makes the
    /// success/failure shape a type-level guarantee; the DB CHECK is defense
    /// in depth. Validates that `seed` fits in `i64` (SQLite INTEGER is
    /// signed) before binding.
    pub fn record_attempt(&self, attempt: &NewAttempt) -> Result<i64, RecordAttemptError> {
        let NewAttempt {
            key,
            finished_at,
            outcome,
        } = attempt;
        if key.seed > i64::MAX as u64 {
            return Err(RecordAttemptError::SeedOutOfRange { value: key.seed });
        }
        let seed_i64 = key.seed as i64;
        let (status, score1, score2, turns, failure_reason, started_at) = match outcome {
            NewAttemptOutcome::Success {
                player1_score,
                player2_score,
                turns,
                started_at,
            } => (
                AttemptStatus::Success.as_str(),
                Some(*player1_score),
                Some(*player2_score),
                Some(*turns),
                None,
                Some(started_at.as_str()),
            ),
            NewAttemptOutcome::Failure {
                failure_reason,
                started_at,
            } => (
                AttemptStatus::Failure.as_str(),
                None,
                None,
                None,
                Some(failure_reason.as_str()),
                started_at.as_deref(),
            ),
        };
        let result = self.conn.execute(
            "INSERT INTO match_attempts
               (tournament_id, game_config_id, player1_id, player2_id, seed,
                repetition_index, attempt_index, status,
                player1_score, player2_score, turns,
                failure_reason, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                key.tournament_id.0,
                key.game_config_id,
                key.player1_id,
                key.player2_id,
                seed_i64,
                key.repetition_index,
                key.attempt_index,
                status,
                score1,
                score2,
                turns,
                failure_reason,
                started_at,
                finished_at,
            ],
        );
        match result {
            Ok(_) => Ok(self.conn.last_insert_rowid()),
            // The only UNIQUE constraint on `match_attempts` is the matchup
            // key tuple; surface it as a typed error so the planner can
            // distinguish retry-collision from a generic DB blip.
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Err(RecordAttemptError::AttemptAlreadyExists { key: key.clone() })
            },
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_attempts(
        &self,
        tournament_id: TournamentId,
        status_filter: Option<AttemptStatus>,
    ) -> Result<Vec<AttemptRecord>, EvalError> {
        let mut sql = String::from(
            "SELECT id, tournament_id, game_config_id, player1_id, player2_id, seed,
                    repetition_index, attempt_index, status,
                    player1_score, player2_score, turns,
                    failure_reason, started_at, finished_at
               FROM match_attempts WHERE tournament_id = ?1",
        );
        if status_filter.is_some() {
            sql.push_str(" AND status = ?2");
        }
        sql.push_str(" ORDER BY id");

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = if let Some(s) = status_filter {
            stmt.query_map(params![tournament_id.0, s.as_str()], read_attempt_row)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![tournament_id.0], read_attempt_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    /// Load this tournament's success attempts and aggregate into pairwise
    /// head-to-head records. The standard Elo input for a tournament; a
    /// thin wrapper over [`get_attempts`] + [`head_to_head_from_attempt_records`].
    pub fn head_to_head_from_attempts(
        &self,
        tournament_id: TournamentId,
    ) -> Result<Vec<HeadToHead>, EvalError> {
        let attempts = self.get_attempts(tournament_id, Some(AttemptStatus::Success))?;
        Ok(head_to_head_from_attempt_records(&attempts))
    }
}

fn read_player_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlayerRecord> {
    Ok(PlayerRecord {
        id: row.get(0)?,
        display_name: row.get(1)?,
        created_at: row.get(2)?,
        agent_id: row.get(3)?,
        version: row.get(4)?,
        command: row.get(5)?,
        metadata_json: row.get(6)?,
    })
}

fn read_tournament_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TournamentRecord> {
    Ok(TournamentRecord {
        id: TournamentId(row.get(0)?),
        format: row.get(1)?,
        target_games_per_matchup: row.get(2)?,
        params_json: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn read_attempt_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttemptRecord> {
    let seed_i64: i64 = row.get(5)?;
    let status_str: String = row.get(8)?;
    let status = AttemptStatus::from_str(&status_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            8,
            rusqlite::types::Type::Text,
            format!("invalid attempt status: {status_str}").into(),
        )
    })?;
    let key = AttemptKey {
        tournament_id: TournamentId(row.get(1)?),
        game_config_id: row.get(2)?,
        player1_id: row.get(3)?,
        player2_id: row.get(4)?,
        seed: seed_i64 as u64,
        repetition_index: row.get(6)?,
        attempt_index: row.get(7)?,
    };
    // The `match_attempts` CHECK guarantees the column shape per status,
    // so the unwraps below cannot fire on a healthy DB.
    let outcome = match status {
        AttemptStatus::Success => AttemptOutcome::Success {
            player1_score: row.get(9)?,
            player2_score: row.get(10)?,
            turns: row.get(11)?,
            started_at: row.get(13)?,
        },
        AttemptStatus::Failure => AttemptOutcome::Failure {
            failure_reason: row.get(12)?,
            started_at: row.get(13)?,
        },
    };
    Ok(AttemptRecord {
        id: row.get(0)?,
        key,
        finished_at: row.get(14)?,
        outcome,
    })
}

fn check_conflict(
    out: &mut Vec<String>,
    field: &str,
    existing: &Option<String>,
    new: &Option<String>,
) {
    if let (Some(e), Some(n)) = (existing, new) {
        if e != n {
            out.push(field.to_string());
        }
    }
}

struct ExistingIdentity {
    agent_id: Option<String>,
    version: Option<String>,
    command: Option<String>,
    metadata_json: Option<String>,
}

/// Aggregate game results into head-to-head records.
/// Groups by (player1, player2) pair, classifies win/loss/draw from scores.
pub fn head_to_head_from_results(results: &[GameResultRecord]) -> Vec<HeadToHead> {
    aggregate_pairs(results.iter().map(|r| {
        (
            &r.player1_id,
            &r.player2_id,
            r.player1_score,
            r.player2_score,
        )
    }))
}

/// Same shape as [`head_to_head_from_results`], but over tournament attempt
/// rows already loaded into memory. Failure rows are skipped — Elo is computed
/// from successful matches only. Variant-dispatch on `outcome` makes the
/// success-only access total.
///
/// For "load and aggregate from the store" in one call, see
/// [`EvalStore::head_to_head_from_attempts`].
pub fn head_to_head_from_attempt_records(attempts: &[AttemptRecord]) -> Vec<HeadToHead> {
    aggregate_pairs(attempts.iter().filter_map(|a| match &a.outcome {
        AttemptOutcome::Success {
            player1_score,
            player2_score,
            ..
        } => Some((
            &a.key.player1_id,
            &a.key.player2_id,
            *player1_score,
            *player2_score,
        )),
        AttemptOutcome::Failure { .. } => None,
    }))
}

fn aggregate_pairs<'a, I>(iter: I) -> Vec<HeadToHead>
where
    I: IntoIterator<Item = (&'a String, &'a String, f64, f64)>,
{
    let mut map: HashMap<(String, String), (u32, u32, u32)> = HashMap::new();
    for (p1, p2, s1, s2) in iter {
        let (a, b, a_score, b_score) = if p1 <= p2 {
            (p1, p2, s1, s2)
        } else {
            (p2, p1, s2, s1)
        };
        let entry = map.entry((a.clone(), b.clone())).or_insert((0, 0, 0));
        if (a_score - b_score).abs() < 1e-9 {
            entry.2 += 1;
        } else if a_score > b_score {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    let mut records: Vec<HeadToHead> = map
        .into_iter()
        .map(|((a, b), (wa, wb, d))| HeadToHead::with_draws(a, b, wa, wb, d))
        .collect();
    records.sort_by(|a, b| (&a.player_a, &a.player_b).cmp(&(&b.player_a, &b.player_b)));
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elo::compute_elo;

    fn sample_config() -> GameConfigRecord {
        GameConfigRecord {
            width: 21,
            height: 15,
            max_turns: 300,
            wall_density: 0.7,
            mud_density: 0.1,
            mud_range: 5,
            connected: true,
            symmetric: true,
            cheese_count: 41,
            cheese_symmetric: true,
        }
    }

    fn setup_players(store: &EvalStore) {
        store.ensure_player("alice", "Alice").unwrap();
        store.ensure_player("bob", "Bob").unwrap();
    }

    fn setup_tournament(store: &EvalStore) -> (TournamentId, String) {
        setup_players(store);
        let config_id = store.ensure_game_config(&sample_config()).unwrap();
        let tid = store
            .create_tournament(&NewTournament {
                format: "round-robin".into(),
                target_games_per_matchup: Some(10),
                params_json: "{}".into(),
            })
            .unwrap();
        store.add_tournament_player(tid, "alice", 0).unwrap();
        store.add_tournament_player(tid, "bob", 1).unwrap();
        (tid, config_id)
    }

    fn success_attempt(
        tid: TournamentId,
        cid: &str,
        p1: &str,
        p2: &str,
        attempt_index: u32,
        s1: f64,
        s2: f64,
    ) -> NewAttempt {
        NewAttempt {
            key: AttemptKey {
                tournament_id: tid,
                game_config_id: cid.into(),
                player1_id: p1.into(),
                player2_id: p2.into(),
                seed: 1234,
                repetition_index: 0,
                attempt_index,
            },
            finished_at: "2026-05-06 10:05:00".into(),
            outcome: NewAttemptOutcome::Success {
                player1_score: s1,
                player2_score: s2,
                turns: 100,
                started_at: "2026-05-06 10:00:00".into(),
            },
        }
    }

    fn failure_attempt(
        tid: TournamentId,
        cid: &str,
        p1: &str,
        p2: &str,
        attempt_index: u32,
        started_at: Option<&str>,
    ) -> NewAttempt {
        NewAttempt {
            key: AttemptKey {
                tournament_id: tid,
                game_config_id: cid.into(),
                player1_id: p1.into(),
                player2_id: p2.into(),
                seed: 5678,
                repetition_index: 0,
                attempt_index,
            },
            finished_at: "2026-05-06 10:10:00".into(),
            outcome: NewAttemptOutcome::Failure {
                failure_reason: "bot crash".into(),
                started_at: started_at.map(String::from),
            },
        }
    }

    #[test]
    fn tables_created_on_open() {
        let store = EvalStore::open_in_memory().unwrap();
        let tables: Vec<String> = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(tables.contains(&"game_configs".to_string()));
        assert!(tables.contains(&"players".to_string()));
        assert!(tables.contains(&"game_results".to_string()));
    }

    #[test]
    fn ensure_player_insert_and_idempotent() {
        let store = EvalStore::open_in_memory().unwrap();
        store.ensure_player("alice", "Alice").unwrap();
        store.ensure_player("alice", "Alice v2").unwrap(); // should not error or update

        let players = store.get_players().unwrap();
        assert_eq!(players.len(), 1);
        assert_eq!(players[0].display_name, "Alice"); // original name kept
    }

    #[test]
    fn ensure_game_config_content_hash_and_dedup() {
        let store = EvalStore::open_in_memory().unwrap();
        let config = sample_config();
        let id1 = store.ensure_game_config(&config).unwrap();
        let id2 = store.ensure_game_config(&config).unwrap();
        assert_eq!(id1, id2);

        let configs = store.get_game_configs().unwrap();
        assert_eq!(configs.len(), 1);
    }

    #[test]
    fn record_result_and_retrieve() {
        let store = EvalStore::open_in_memory().unwrap();
        setup_players(&store);
        let config_id = store.ensure_game_config(&sample_config()).unwrap();

        let result_id = store
            .record_result(&NewGameResult {
                game_config_id: config_id.clone(),
                player1_id: "alice".into(),
                player2_id: "bob".into(),
                player1_score: 12.0,
                player2_score: 9.0,
                turns: 150,
            })
            .unwrap();
        assert_eq!(result_id, 1);

        let results = store.get_results(&ResultFilter::default()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].player1_score, 12.0);
        assert_eq!(results[0].player2_score, 9.0);
        assert_eq!(results[0].turns, 150);
    }

    #[test]
    fn filter_by_player() {
        let store = EvalStore::open_in_memory().unwrap();
        setup_players(&store);
        store.ensure_player("carol", "Carol").unwrap();
        let config_id = store.ensure_game_config(&sample_config()).unwrap();

        store
            .record_result(&NewGameResult {
                game_config_id: config_id.clone(),
                player1_id: "alice".into(),
                player2_id: "bob".into(),
                player1_score: 10.0,
                player2_score: 11.0,
                turns: 200,
            })
            .unwrap();

        store
            .record_result(&NewGameResult {
                game_config_id: config_id.clone(),
                player1_id: "carol".into(),
                player2_id: "bob".into(),
                player1_score: 8.0,
                player2_score: 13.0,
                turns: 180,
            })
            .unwrap();

        let alice_results = store
            .get_results(&ResultFilter {
                player_id: Some("alice".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(alice_results.len(), 1);

        let bob_results = store
            .get_results(&ResultFilter {
                player_id: Some("bob".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(bob_results.len(), 2);
    }

    #[test]
    fn filter_by_config() {
        let store = EvalStore::open_in_memory().unwrap();
        setup_players(&store);

        let config_a = sample_config();
        let config_b = GameConfigRecord {
            width: 11,
            height: 11,
            ..sample_config()
        };
        let id_a = store.ensure_game_config(&config_a).unwrap();
        let id_b = store.ensure_game_config(&config_b).unwrap();

        for config_id in [&id_a, &id_b] {
            store
                .record_result(&NewGameResult {
                    game_config_id: config_id.clone(),
                    player1_id: "alice".into(),
                    player2_id: "bob".into(),
                    player1_score: 10.0,
                    player2_score: 11.0,
                    turns: 100,
                })
                .unwrap();
        }

        let results = store
            .get_results(&ResultFilter {
                game_config_id: Some(id_a.clone()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].game_config_id, id_a);
    }

    #[test]
    fn filter_by_date_range() {
        let store = EvalStore::open_in_memory().unwrap();
        setup_players(&store);
        let config_id = store.ensure_game_config(&sample_config()).unwrap();

        store.conn.execute(
            "INSERT INTO game_results (game_config_id, player1_id, player2_id, player1_score, player2_score, turns, played_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![config_id, "alice", "bob", 10.0, 11.0, 100, "2025-01-01 00:00:00"],
        ).unwrap();
        store.conn.execute(
            "INSERT INTO game_results (game_config_id, player1_id, player2_id, player1_score, player2_score, turns, played_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![config_id, "alice", "bob", 12.0, 9.0, 200, "2025-06-01 00:00:00"],
        ).unwrap();

        let results = store
            .get_results(&ResultFilter {
                after: Some("2025-03-01 00:00:00".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].player1_score, 12.0);

        let results = store
            .get_results(&ResultFilter {
                before: Some("2025-03-01 00:00:00".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].player1_score, 10.0);
    }

    #[test]
    fn delete_player_cascades_results() {
        let store = EvalStore::open_in_memory().unwrap();
        setup_players(&store);
        let config_id = store.ensure_game_config(&sample_config()).unwrap();

        store
            .record_result(&NewGameResult {
                game_config_id: config_id,
                player1_id: "alice".into(),
                player2_id: "bob".into(),
                player1_score: 10.0,
                player2_score: 11.0,
                turns: 100,
            })
            .unwrap();

        assert!(store.delete_player("alice").unwrap());
        let results = store.get_results(&ResultFilter::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn delete_nonexistent_returns_false() {
        let store = EvalStore::open_in_memory().unwrap();
        assert!(!store.delete_player("ghost").unwrap());
    }

    #[test]
    fn different_configs_different_hashes() {
        let a = sample_config();
        let b = GameConfigRecord {
            width: 11,
            ..sample_config()
        };
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn config_roundtrip() {
        let store = EvalStore::open_in_memory().unwrap();
        let original = sample_config();
        let id = store.ensure_game_config(&original).unwrap();

        let configs = store.get_game_configs().unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].0, id);
        assert_eq!(configs[0].1, original);
    }

    #[test]
    fn foreign_key_enforcement() {
        let store = EvalStore::open_in_memory().unwrap();

        let err = store.record_result(&NewGameResult {
            game_config_id: "nonexistent".into(),
            player1_id: "alice".into(),
            player2_id: "bob".into(),
            player1_score: 0.0,
            player2_score: 0.0,
            turns: 0,
        });
        assert!(err.is_err());

        let config_id = store.ensure_game_config(&sample_config()).unwrap();
        let err = store.record_result(&NewGameResult {
            game_config_id: config_id,
            player1_id: "nonexistent".into(),
            player2_id: "also_nonexistent".into(),
            player1_score: 0.0,
            player2_score: 0.0,
            turns: 0,
        });
        assert!(err.is_err());
    }

    #[test]
    fn open_file_backed_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eval.db");

        {
            let store = EvalStore::open(&path).unwrap();
            store.ensure_player("alice", "Alice").unwrap();
        }

        {
            let store = EvalStore::open(&path).unwrap();
            let players = store.get_players().unwrap();
            assert_eq!(players.len(), 1);
            assert_eq!(players[0].id, "alice");
        }
    }

    fn user_version(store: &EvalStore) -> u32 {
        store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn migration_fresh_db_ends_at_user_version_2() {
        let store = EvalStore::open_in_memory().unwrap();
        assert_eq!(user_version(&store), 2);

        let tables: Vec<String> = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for expected in [
            "game_configs",
            "game_results",
            "match_attempts",
            "players",
            "tournament_players",
            "tournaments",
        ] {
            assert!(
                tables.contains(&expected.to_string()),
                "missing table: {expected}; have {tables:?}"
            );
        }
    }

    #[test]
    fn migration_upgrades_handbuilt_v0_db() {
        // Build a connection that has migration-1 tables but `user_version=0`,
        // mimicking a DB created before the migration runner existed.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn.execute_batch(
            "CREATE TABLE players (id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
                                   created_at TEXT NOT NULL DEFAULT (datetime('now')));
             CREATE TABLE game_configs (id TEXT PRIMARY KEY, config_json TEXT NOT NULL);
             CREATE TABLE game_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                game_config_id TEXT NOT NULL REFERENCES game_configs(id),
                player1_id TEXT NOT NULL REFERENCES players(id) ON DELETE CASCADE,
                player2_id TEXT NOT NULL REFERENCES players(id) ON DELETE CASCADE,
                player1_score REAL NOT NULL, player2_score REAL NOT NULL,
                turns INTEGER NOT NULL,
                played_at TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )
        .unwrap();
        // user_version is 0 by default — explicitly assert.
        let v: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(v, 0);

        let store = EvalStore::from_connection(conn).unwrap();
        assert_eq!(user_version(&store), 2);

        // Migration 1 (CREATE IF NOT EXISTS) is a no-op; migration 2 adds
        // the new tables and columns. Confirm the v2 surface is present.
        let cols: Vec<String> = store
            .conn
            .prepare("SELECT name FROM pragma_table_info('players')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for expected in ["agent_id", "version", "command", "metadata_json"] {
            assert!(
                cols.contains(&expected.to_string()),
                "migration 2 should have added {expected} column to players: have {cols:?}"
            );
        }
    }

    #[test]
    fn migration_idempotent_on_second_run() {
        let mut conn = Connection::open_in_memory().unwrap();
        schema::initialize(&mut conn).unwrap();
        let v: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(v, 2);
        // Second run on the same connection: each migration's `version > current`
        // guard makes the loop a no-op. Must not error.
        schema::initialize(&mut conn).unwrap();
        let v: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(v, 2);
    }

    #[test]
    fn pragma_foreign_keys_is_on_per_connection() {
        let store = EvalStore::open_in_memory().unwrap();
        let fk: i64 = store
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn duplicate_attempt_key_violates_unique() {
        let store = EvalStore::open_in_memory().unwrap();
        let (tid, cid) = setup_tournament(&store);
        store
            .record_attempt(&success_attempt(tid, &cid, "alice", "bob", 0, 8.0, 2.0))
            .unwrap();
        let dup = store.record_attempt(&success_attempt(tid, &cid, "alice", "bob", 0, 1.0, 9.0));
        match dup {
            Err(RecordAttemptError::AttemptAlreadyExists { key }) => {
                assert_eq!(key.player1_id, "alice");
                assert_eq!(key.player2_id, "bob");
                assert_eq!(key.attempt_index, 0);
                assert_eq!(key.repetition_index, 0);
            },
            other => panic!("expected AttemptAlreadyExists, got {other:?}"),
        }
    }

    #[test]
    fn record_attempt_seed_bound() {
        let store = EvalStore::open_in_memory().unwrap();
        let (tid, cid) = setup_tournament(&store);
        // Out of range: rejected before binding.
        let mut bad = success_attempt(tid, &cid, "alice", "bob", 0, 8.0, 2.0);
        bad.key.seed = u64::MAX;
        match store.record_attempt(&bad) {
            Err(RecordAttemptError::SeedOutOfRange { value }) => assert_eq!(value, u64::MAX),
            other => panic!("expected SeedOutOfRange, got {other:?}"),
        }
        // i64::MAX round-trips exactly.
        let mut at_max = success_attempt(tid, &cid, "alice", "bob", 1, 5.0, 5.0);
        at_max.key.seed = i64::MAX as u64;
        store.record_attempt(&at_max).unwrap();
        let attempts = store.get_attempts(tid, None).unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].key.seed, i64::MAX as u64);
    }

    #[test]
    fn check_rejects_malformed_success_row() {
        // Bypass the typed API; DB CHECK is the last line of defense.
        let store = EvalStore::open_in_memory().unwrap();
        let (tid, cid) = setup_tournament(&store);
        let res = store.conn.execute(
            "INSERT INTO match_attempts
              (tournament_id, game_config_id, player1_id, player2_id, seed,
               repetition_index, attempt_index, status,
               player1_score, player2_score, turns, failure_reason, started_at,
               finished_at)
             VALUES (?1, ?2, 'alice', 'bob', 1, 0, 0, 'success',
                     NULL, 5.0, 100, NULL, '2026-01-01 00:00:00',
                     '2026-01-01 00:05:00')",
            params![tid.0, cid],
        );
        assert!(
            res.is_err(),
            "success row missing player1_score must fail CHECK"
        );

        let res = store.conn.execute(
            "INSERT INTO match_attempts
              (tournament_id, game_config_id, player1_id, player2_id, seed,
               repetition_index, attempt_index, status,
               player1_score, player2_score, turns, failure_reason, started_at,
               finished_at)
             VALUES (?1, ?2, 'alice', 'bob', 1, 0, 1, 'success',
                     5.0, 5.0, 100, NULL, NULL,
                     '2026-01-01 00:05:00')",
            params![tid.0, cid],
        );
        assert!(
            res.is_err(),
            "success row with NULL started_at must fail CHECK"
        );
    }

    #[test]
    fn check_rejects_malformed_failure_row() {
        let store = EvalStore::open_in_memory().unwrap();
        let (tid, cid) = setup_tournament(&store);
        let res = store.conn.execute(
            "INSERT INTO match_attempts
              (tournament_id, game_config_id, player1_id, player2_id, seed,
               repetition_index, attempt_index, status,
               player1_score, player2_score, turns, failure_reason, started_at,
               finished_at)
             VALUES (?1, ?2, 'alice', 'bob', 1, 0, 0, 'failure',
                     5.0, NULL, NULL, 'crash', NULL,
                     '2026-01-01 00:05:00')",
            params![tid.0, cid],
        );
        assert!(res.is_err(), "failure row with score must fail CHECK");

        let res = store.conn.execute(
            "INSERT INTO match_attempts
              (tournament_id, game_config_id, player1_id, player2_id, seed,
               repetition_index, attempt_index, status,
               player1_score, player2_score, turns, failure_reason, started_at,
               finished_at)
             VALUES (?1, ?2, 'alice', 'bob', 1, 0, 1, 'failure',
                     NULL, NULL, NULL, NULL, NULL,
                     '2026-01-01 00:05:00')",
            params![tid.0, cid],
        );
        assert!(
            res.is_err(),
            "failure row missing failure_reason must fail CHECK"
        );
    }

    #[test]
    fn get_attempts_returns_mixed_status() {
        let store = EvalStore::open_in_memory().unwrap();
        let (tid, cid) = setup_tournament(&store);
        store
            .record_attempt(&success_attempt(tid, &cid, "alice", "bob", 0, 8.0, 2.0))
            .unwrap();
        store
            .record_attempt(&failure_attempt(
                tid,
                &cid,
                "alice",
                "bob",
                1,
                Some("2026-05-06 11:00:00"),
            ))
            .unwrap();

        let all = store.get_attempts(tid, None).unwrap();
        assert_eq!(all.len(), 2);
        let success_count = all
            .iter()
            .filter(|a| a.status() == AttemptStatus::Success)
            .count();
        let failure_count = all
            .iter()
            .filter(|a| a.status() == AttemptStatus::Failure)
            .count();
        assert_eq!(success_count, 1);
        assert_eq!(failure_count, 1);

        let succ = store
            .get_attempts(tid, Some(AttemptStatus::Success))
            .unwrap();
        assert_eq!(succ.len(), 1);
        let fail = store
            .get_attempts(tid, Some(AttemptStatus::Failure))
            .unwrap();
        assert_eq!(fail.len(), 1);
    }

    #[test]
    fn h2h_from_attempts_skips_failures() {
        let store = EvalStore::open_in_memory().unwrap();
        let (tid, cid) = setup_tournament(&store);
        store
            .record_attempt(&success_attempt(tid, &cid, "alice", "bob", 0, 8.0, 2.0))
            .unwrap();
        store
            .record_attempt(&failure_attempt(tid, &cid, "alice", "bob", 1, None))
            .unwrap();

        // Free-fn path: caller already holds the records.
        let attempts = store.get_attempts(tid, None).unwrap();
        let h = head_to_head_from_attempt_records(&attempts);
        assert_eq!(h.len(), 1);
        // alice (player_a) won the only success; failure ignored.
        assert_eq!(h[0].wins_a + h[0].wins_b + h[0].draws, 1);

        // Store-method path: same result, one call.
        let h2 = store.head_to_head_from_attempts(tid).unwrap();
        assert_eq!(h2, h);
    }

    #[test]
    fn spawn_failure_attempt_roundtrips() {
        let store = EvalStore::open_in_memory().unwrap();
        let (tid, cid) = setup_tournament(&store);
        store
            .record_attempt(&failure_attempt(tid, &cid, "alice", "bob", 0, None))
            .unwrap();

        let attempts = store.get_attempts(tid, None).unwrap();
        assert_eq!(attempts.len(), 1);
        let a = &attempts[0];
        assert_eq!(a.status(), AttemptStatus::Failure);
        match &a.outcome {
            AttemptOutcome::Failure {
                failure_reason,
                started_at,
            } => {
                assert!(started_at.is_none(), "spawn-failure has no started_at");
                assert_eq!(failure_reason.as_str(), "bot crash");
            },
            other => panic!("expected Failure outcome, got {other:?}"),
        }
    }

    #[test]
    fn add_tournament_player_distinct_conflict_errors() {
        let store = EvalStore::open_in_memory().unwrap();
        setup_players(&store);
        store.ensure_player("carol", "Carol").unwrap();
        let tid = store
            .create_tournament(&NewTournament {
                format: "round-robin".into(),
                target_games_per_matchup: None,
                params_json: "{}".into(),
            })
            .unwrap();
        store.add_tournament_player(tid, "alice", 0).unwrap();

        // Same player twice → PlayerAlreadyInTournament.
        match store.add_tournament_player(tid, "alice", 5) {
            Err(AddTournamentPlayerError::PlayerAlreadyInTournament { player_id, .. }) => {
                assert_eq!(player_id, "alice")
            },
            other => panic!("expected PlayerAlreadyInTournament, got {other:?}"),
        }

        // Different player, same slot → SlotTaken.
        match store.add_tournament_player(tid, "carol", 0) {
            Err(AddTournamentPlayerError::SlotTaken { slot, .. }) => assert_eq!(slot, 0),
            other => panic!("expected SlotTaken, got {other:?}"),
        }
    }

    #[test]
    fn delete_player_blocked_by_tournament_history() {
        let store = EvalStore::open_in_memory().unwrap();
        let (tid, cid) = setup_tournament(&store);
        store
            .record_attempt(&success_attempt(tid, &cid, "alice", "bob", 0, 8.0, 2.0))
            .unwrap();

        match store.delete_player("alice") {
            Err(DeletePlayerError::InTournamentHistory { tournament_ids }) => {
                assert_eq!(tournament_ids, vec![tid]);
            },
            other => panic!("expected InTournamentHistory, got {other:?}"),
        }
    }

    #[test]
    fn delete_player_succeeds_when_only_ad_hoc_results() {
        let store = EvalStore::open_in_memory().unwrap();
        setup_players(&store);
        let cid = store.ensure_game_config(&sample_config()).unwrap();
        store
            .record_result(&NewGameResult {
                game_config_id: cid,
                player1_id: "alice".into(),
                player2_id: "bob".into(),
                player1_score: 5.0,
                player2_score: 5.0,
                turns: 100,
            })
            .unwrap();

        // No tournament rows; deletion succeeds and cascades game_results.
        assert!(store.delete_player("alice").unwrap());
        let results = store.get_results(&ResultFilter::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn delete_tournament_cascades_children_only() {
        let store = EvalStore::open_in_memory().unwrap();
        let (tid, cid) = setup_tournament(&store);
        store
            .record_attempt(&success_attempt(tid, &cid, "alice", "bob", 0, 8.0, 2.0))
            .unwrap();

        // Add an unrelated tournament so we can prove only the targeted one
        // is touched.
        let other = store
            .create_tournament(&NewTournament {
                format: "gauntlet".into(),
                target_games_per_matchup: None,
                params_json: "{}".into(),
            })
            .unwrap();
        store.add_tournament_player(other, "alice", 0).unwrap();
        store.add_tournament_player(other, "bob", 1).unwrap();
        store
            .record_attempt(&success_attempt(other, &cid, "alice", "bob", 0, 6.0, 4.0))
            .unwrap();

        // Also a stray ad-hoc row to confirm it survives.
        store
            .record_result(&NewGameResult {
                game_config_id: cid.clone(),
                player1_id: "alice".into(),
                player2_id: "bob".into(),
                player1_score: 3.0,
                player2_score: 7.0,
                turns: 50,
            })
            .unwrap();

        assert!(store.delete_tournament(tid).unwrap());

        // Targeted tournament: rows gone.
        assert!(store.get_attempts(tid, None).unwrap().is_empty());
        assert!(store.get_tournament_players(tid).unwrap().is_empty());
        assert!(store.get_tournament(tid).unwrap().is_none());

        // Other tournament + ad-hoc rows + players: untouched.
        assert_eq!(store.get_attempts(other, None).unwrap().len(), 1);
        assert_eq!(store.get_tournament_players(other).unwrap().len(), 2);
        assert_eq!(
            store.get_results(&ResultFilter::default()).unwrap().len(),
            1
        );
        assert_eq!(store.get_players().unwrap().len(), 2);
    }

    #[test]
    fn delete_tournament_unblocks_delete_player() {
        let store = EvalStore::open_in_memory().unwrap();
        let (tid, cid) = setup_tournament(&store);
        store
            .record_attempt(&success_attempt(tid, &cid, "alice", "bob", 0, 8.0, 2.0))
            .unwrap();

        assert!(store.delete_player("alice").is_err());
        store.delete_tournament(tid).unwrap();
        assert!(store.delete_player("alice").unwrap());
    }

    #[test]
    fn register_player_conflict_fill_idempotent() {
        let store = EvalStore::open_in_memory().unwrap();

        // Conflict path: insert with version=1, attempt to re-insert with
        // version=2 → IdentityConflict listing version.
        store
            .register_player(&NewPlayer {
                id: "pyrat/greedy".into(),
                display_name: "Greedy".into(),
                agent_id: Some("pyrat/greedy".into()),
                version: Some("1".into()),
                command: Some("cargo run".into()),
                metadata_json: None,
            })
            .unwrap();
        match store.register_player(&NewPlayer {
            id: "pyrat/greedy".into(),
            display_name: "Greedy".into(),
            agent_id: Some("pyrat/greedy".into()),
            version: Some("2".into()),
            command: Some("cargo run".into()),
            metadata_json: None,
        }) {
            Err(RegisterPlayerError::IdentityConflict { id, fields }) => {
                assert_eq!(id, "pyrat/greedy");
                assert_eq!(fields, vec!["version".to_string()]);
            },
            other => panic!("expected IdentityConflict, got {other:?}"),
        }

        // Idempotent path: identical re-insert is a no-op success.
        store
            .register_player(&NewPlayer {
                id: "pyrat/greedy".into(),
                display_name: "Greedy".into(),
                agent_id: Some("pyrat/greedy".into()),
                version: Some("1".into()),
                command: Some("cargo run".into()),
                metadata_json: None,
            })
            .unwrap();

        // NULL-fill path: ensure_player creates a row without identity columns;
        // register_player fills them in.
        store.ensure_player("legacy", "Legacy").unwrap();
        store
            .register_player(&NewPlayer {
                id: "legacy".into(),
                display_name: "Legacy".into(),
                agent_id: Some("pyrat/legacy".into()),
                version: Some("1".into()),
                command: None,
                metadata_json: Some(r#"{"note":"backfill"}"#.into()),
            })
            .unwrap();
        let p = store.get_player("legacy").unwrap().unwrap();
        assert_eq!(p.agent_id.as_deref(), Some("pyrat/legacy"));
        assert_eq!(p.version.as_deref(), Some("1"));
        assert!(p.command.is_none()); // wasn't supplied, stays NULL
        assert_eq!(p.metadata_json.as_deref(), Some(r#"{"note":"backfill"}"#));
    }

    #[test]
    fn get_player_returns_some_or_none() {
        let store = EvalStore::open_in_memory().unwrap();
        store
            .register_player(&NewPlayer {
                id: "alice".into(),
                display_name: "Alice".into(),
                agent_id: Some("pyrat/alice".into()),
                version: None,
                command: None,
                metadata_json: None,
            })
            .unwrap();
        let p = store.get_player("alice").unwrap().unwrap();
        assert_eq!(p.id, "alice");
        assert_eq!(p.agent_id.as_deref(), Some("pyrat/alice"));
        assert!(store.get_player("ghost").unwrap().is_none());
    }

    fn game(id: i64, p1: &str, p2: &str, s1: f64, s2: f64) -> GameResultRecord {
        GameResultRecord {
            id,
            game_config_id: "c".into(),
            player1_id: p1.into(),
            player2_id: p2.into(),
            player1_score: s1,
            player2_score: s2,
            turns: 100,
            played_at: "t".into(),
        }
    }

    #[test]
    fn h2h_classifies_wins_losses_draws() {
        let results = vec![
            game(1, "A", "B", 5.0, 3.0),
            game(2, "B", "A", 4.0, 4.0),
            game(3, "A", "B", 2.0, 6.0),
        ];
        let h = head_to_head_from_results(&results);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].player_a, "A");
        assert_eq!(h[0].player_b, "B");
        assert_eq!(h[0].wins_a, 1);
        assert_eq!(h[0].wins_b, 1);
        assert_eq!(h[0].draws, 1);
    }

    #[test]
    fn h2h_groups_by_pair() {
        let results = vec![game(1, "A", "B", 5.0, 3.0), game(2, "A", "C", 4.0, 4.0)];
        let h = head_to_head_from_results(&results);
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn full_pipeline_results_to_elo() {
        use crate::elo::EloOptions;

        let results = vec![
            game(1, "greedy", "random", 8.0, 2.0),
            game(2, "greedy", "random", 7.0, 3.0),
            game(3, "random", "greedy", 4.0, 6.0),
        ];
        let h = head_to_head_from_results(&results);
        let result = compute_elo(&h, &EloOptions::new("random")).unwrap();
        let greedy_elo = result.get_elo("greedy").unwrap();
        let random_elo = result.get_elo("random").unwrap();
        assert!(greedy_elo > random_elo, "greedy should be rated higher");
        assert!((random_elo - 1000.0).abs() < 0.01, "anchor should be exact");
    }
}
