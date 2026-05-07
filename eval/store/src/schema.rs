use rusqlite::Connection;

use crate::EvalError;

const MIGRATION_1: &str = "
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

CREATE INDEX IF NOT EXISTS idx_results_player1 ON game_results(player1_id);
CREATE INDEX IF NOT EXISTS idx_results_player2 ON game_results(player2_id);
CREATE INDEX IF NOT EXISTS idx_results_config  ON game_results(game_config_id);
CREATE INDEX IF NOT EXISTS idx_results_played  ON game_results(played_at);
";

const MIGRATION_2: &str = "
ALTER TABLE players ADD COLUMN agent_id      TEXT;
ALTER TABLE players ADD COLUMN version       TEXT;
ALTER TABLE players ADD COLUMN command       TEXT;
ALTER TABLE players ADD COLUMN metadata_json TEXT;

CREATE TABLE tournaments (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    format                   TEXT    NOT NULL,
    target_games_per_matchup INTEGER,
    params_json              TEXT    NOT NULL,
    created_at               TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE tournament_players (
    tournament_id INTEGER NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
    player_id     TEXT    NOT NULL REFERENCES players(id) ON DELETE RESTRICT,
    slot          INTEGER NOT NULL,
    PRIMARY KEY (tournament_id, player_id),
    UNIQUE (tournament_id, slot)
);

CREATE TABLE match_attempts (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    tournament_id    INTEGER NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
    game_config_id   TEXT    NOT NULL REFERENCES game_configs(id),
    player1_id       TEXT    NOT NULL REFERENCES players(id) ON DELETE RESTRICT,
    player2_id       TEXT    NOT NULL REFERENCES players(id) ON DELETE RESTRICT,
    seed             INTEGER NOT NULL,
    repetition_index INTEGER NOT NULL DEFAULT 0,
    attempt_index    INTEGER NOT NULL,
    status           TEXT    NOT NULL,
    player1_score    REAL,
    player2_score    REAL,
    turns            INTEGER,
    failure_reason   TEXT,
    started_at       TEXT,
    finished_at      TEXT NOT NULL,
    UNIQUE (tournament_id, game_config_id, player1_id, player2_id, repetition_index, attempt_index),
    CHECK (status IN ('success', 'failure')),
    CHECK (
        (status = 'success' AND player1_score IS NOT NULL
                            AND player2_score IS NOT NULL
                            AND turns          IS NOT NULL
                            AND failure_reason IS NULL
                            AND started_at     IS NOT NULL)
     OR (status = 'failure' AND failure_reason IS NOT NULL
                            AND player1_score  IS NULL
                            AND player2_score  IS NULL
                            AND turns          IS NULL)
    )
);

CREATE INDEX idx_attempts_tournament ON match_attempts(tournament_id);
CREATE INDEX idx_attempts_matchup    ON match_attempts(tournament_id, player1_id, player2_id);
";

const MIGRATIONS: &[(u32, &str)] = &[(1, MIGRATION_1), (2, MIGRATION_2)];

pub fn initialize(conn: &mut Connection) -> Result<(), EvalError> {
    // PRAGMAs are per-connection. `foreign_keys` cannot be set inside a
    // transaction, so apply both before the migration loop opens any.
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

    let current: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for &(version, sql) in MIGRATIONS {
        if version > current {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
            tx.commit()?;
        }
    }
    Ok(())
}
