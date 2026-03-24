use std::path::Path;

use rusqlite::{params, Connection};

use crate::schema;
use crate::types::{
    EvalError, GameConfigRecord, GameResultRecord, NewGameResult, PlayerRecord, ResultFilter,
};

pub struct EvalStore {
    conn: Connection,
}

impl EvalStore {
    /// Open (or create) a store at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EvalError> {
        let conn = Connection::open(path)?;
        schema::initialize(&conn)?;
        Ok(Self { conn })
    }

    /// In-memory store for tests.
    pub fn open_in_memory() -> Result<Self, EvalError> {
        let conn = Connection::open_in_memory()?;
        schema::initialize(&conn)?;
        Ok(Self { conn })
    }

    /// Insert a player if it doesn't already exist.
    pub fn ensure_player(&self, id: &str, display_name: &str) -> Result<(), EvalError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO players (id, display_name) VALUES (?1, ?2)",
            params![id, display_name],
        )?;
        Ok(())
    }

    /// Insert a game config (keyed by content hash) if it doesn't already exist.
    /// Returns the content hash used as the ID.
    pub fn ensure_game_config(&self, config: &GameConfigRecord) -> Result<String, EvalError> {
        let id = config.content_hash();
        let json = serde_json::to_string(config)?;
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

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// List all players.
    pub fn get_players(&self) -> Result<Vec<PlayerRecord>, EvalError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, display_name, created_at FROM players ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok(PlayerRecord {
                id: row.get(0)?,
                display_name: row.get(1)?,
                created_at: row.get(2)?,
            })
        })?;

        let mut players = Vec::new();
        for row in rows {
            players.push(row?);
        }
        Ok(players)
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

        let mut configs = Vec::new();
        for row in rows {
            let (id, json) = row?;
            let config: GameConfigRecord = serde_json::from_str(&json)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            configs.push((id, config));
        }
        Ok(configs)
    }

    /// Delete a player and cascade to their game results.
    /// Returns true if the player existed.
    pub fn delete_player(&self, id: &str) -> Result<bool, EvalError> {
        let deleted = self
            .conn
            .execute("DELETE FROM players WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // alice vs bob
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

        // carol vs bob
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

        // Insert with explicit timestamps
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

        // Bad config ref
        let err = store.record_result(&NewGameResult {
            game_config_id: "nonexistent".into(),
            player1_id: "alice".into(),
            player2_id: "bob".into(),
            player1_score: 0.0,
            player2_score: 0.0,
            turns: 0,
        });
        assert!(err.is_err());

        // Bad player ref (valid config, bad player)
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

        // Create and populate
        {
            let store = EvalStore::open(&path).unwrap();
            store.ensure_player("alice", "Alice").unwrap();
        }

        // Reopen and verify
        {
            let store = EvalStore::open(&path).unwrap();
            let players = store.get_players().unwrap();
            assert_eq!(players.len(), 1);
            assert_eq!(players[0].id, "alice");
        }
    }
}
