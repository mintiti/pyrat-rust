use rusqlite::Connection;

use crate::EvalError;

const DDL: &str = "
CREATE TABLE IF NOT EXISTS game_configs (
    id          TEXT PRIMARY KEY,
    config_json TEXT NOT NULL,
    width       INTEGER GENERATED ALWAYS AS (json_extract(config_json, '$.width')) STORED,
    height      INTEGER GENERATED ALWAYS AS (json_extract(config_json, '$.height')) STORED,
    has_mud     BOOLEAN GENERATED ALWAYS AS (json_extract(config_json, '$.mud_density') > 0) STORED,
    has_walls   BOOLEAN GENERATED ALWAYS AS (json_extract(config_json, '$.wall_density') > 0) STORED,
    symmetric   BOOLEAN GENERATED ALWAYS AS (json_extract(config_json, '$.symmetric')) STORED
);

CREATE TABLE IF NOT EXISTS players (
    id           TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS game_results (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    game_config_id  TEXT NOT NULL REFERENCES game_configs(id),
    player1_id      TEXT NOT NULL REFERENCES players(id) ON DELETE CASCADE,
    player2_id      TEXT NOT NULL REFERENCES players(id) ON DELETE CASCADE,
    player1_score   REAL NOT NULL,
    player2_score   REAL NOT NULL,
    turns           INTEGER NOT NULL,
    played_at       TEXT NOT NULL DEFAULT (datetime('now'))
);
";

pub fn initialize(conn: &Connection) -> Result<(), EvalError> {
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    conn.execute_batch(DDL)?;
    Ok(())
}
