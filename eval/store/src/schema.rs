// Migration 3 rebuilds the tournaments tables to add `game_config_id` and
// `tournament_seed` columns. The session crate needs these so resume can
// validate the planner against the stored tournament identity.
//
// SQLite's `ALTER TABLE ADD COLUMN` cannot add a `NOT NULL REFERENCES`
// column (NOT NULL requires a non-NULL default; REFERENCES requires a NULL
// default). The canonical alternative is to rebuild the table.
//
// Because the dependent tables (`match_attempts`, `tournament_players`) are
// also empty when `tournaments` is empty (their FKs prevent orphan rows),
// dropping + recreating all three in one transaction is safe with foreign
// keys enabled. No PRAGMA toggle needed for the empty-table case.

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

const MIGRATION_3: &str = "
DROP TABLE match_attempts;
DROP TABLE tournament_players;
DROP TABLE tournaments;

CREATE TABLE tournaments (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    format                   TEXT    NOT NULL,
    target_games_per_matchup INTEGER,
    params_json              TEXT    NOT NULL,
    game_config_id           TEXT    NOT NULL REFERENCES game_configs(id),
    tournament_seed          INTEGER NOT NULL,
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

// Migration 4 adds an optional display name to tournaments. Nullable, so a
// plain ADD COLUMN works (no rebuild): rows created without one — e.g. by the
// CLI, which has no name flag — keep NULL. The GUI writes a human name
// ("ckpt-1200") so anything reading the store can tell tournaments apart.
const MIGRATION_4: &str = "
ALTER TABLE tournaments ADD COLUMN name TEXT;
";

// Migration 5 adds the seat-orientation marker to match_attempts. Records
// which player was Rat (slot 0); scores stay canonical, so this is purely
// informational (auditing, replay-correct rendering). Nullable-free ADD
// COLUMN with `DEFAULT 0` (= lex-min was Rat = `SeatOrientation::Canonical`),
// which is exactly how every pre-migration row was played — no rebuild, no
// pre-flight check.
const MIGRATION_5: &str = "
ALTER TABLE match_attempts ADD COLUMN orientation INTEGER NOT NULL DEFAULT 0;
";

// Migration 6 persists the orchestrator match id beside each durable attempt.
// It is deliberately nullable: pre-migration attempts remain valid and fully
// usable for standings/pair inspection, but cannot be joined to a replay file
// by id. The field is informational and does not change attempt identity.
const MIGRATION_6: &str = "
ALTER TABLE match_attempts ADD COLUMN match_id INTEGER;
";

// Migration 7 makes the execution methodology durable on the tournament row.
// Every column is nullable so older rows remain valid and honestly read as
// "unknown"; current writers populate the complete set atomically. The final
// column's CHECK prevents a partial methodology from masquerading as a
// complete one, while the typed read boundary validates the enum/ranges too.
const MIGRATION_7: &str = "
ALTER TABLE tournaments ADD COLUMN timing_mode INTEGER
    CHECK (timing_mode IS NULL OR timing_mode IN (0, 1));
ALTER TABLE tournaments ADD COLUMN move_timeout_ms INTEGER
    CHECK (move_timeout_ms IS NULL OR move_timeout_ms BETWEEN 0 AND 4294967295);
ALTER TABLE tournaments ADD COLUMN preprocessing_timeout_ms INTEGER
    CHECK (preprocessing_timeout_ms IS NULL OR preprocessing_timeout_ms BETWEEN 0 AND 4294967295);
ALTER TABLE tournaments ADD COLUMN startup_timeout_ms INTEGER
    CHECK (startup_timeout_ms IS NULL OR startup_timeout_ms BETWEEN 0 AND 4294967295);
ALTER TABLE tournaments ADD COLUMN configure_timeout_ms INTEGER
    CHECK (configure_timeout_ms IS NULL OR configure_timeout_ms BETWEEN 0 AND 4294967295);
ALTER TABLE tournaments ADD COLUMN network_grace_ms INTEGER
    CHECK (network_grace_ms IS NULL OR network_grace_ms BETWEEN 0 AND 4294967295);
ALTER TABLE tournaments ADD COLUMN max_parallel INTEGER
    CHECK (
        (
            timing_mode IS NULL
            AND move_timeout_ms IS NULL
            AND preprocessing_timeout_ms IS NULL
            AND startup_timeout_ms IS NULL
            AND configure_timeout_ms IS NULL
            AND network_grace_ms IS NULL
            AND max_parallel IS NULL
        )
        OR
        (
            timing_mode IS NOT NULL
            AND move_timeout_ms IS NOT NULL
            AND preprocessing_timeout_ms IS NOT NULL
            AND startup_timeout_ms IS NOT NULL
            AND configure_timeout_ms IS NOT NULL
            AND network_grace_ms IS NOT NULL
            AND max_parallel BETWEEN 1 AND 4294967295
        )
    );
";

// Migration 8 gives tournament rows durable lifecycle/evidence truth and
// gives new failure rows a typed projection. All tournament defaults are
// deliberately `legacy_unknown`: upgrading an old row must not invent
// whether it completed, was stopped, or crashed. Likewise, typed failure
// columns stay NULL for old attempts; their exact `failure_reason` remains
// available to compatibility readers.
const MIGRATION_8: &str = "
ALTER TABLE tournaments ADD COLUMN lifecycle_status TEXT NOT NULL DEFAULT 'legacy_unknown'
    CHECK (lifecycle_status IN (
        'legacy_unknown', 'preparing', 'running', 'completed', 'stopped', 'failed'
    ));
ALTER TABLE tournaments ADD COLUMN started_at TEXT;
ALTER TABLE tournaments ADD COLUMN terminal_at TEXT;
ALTER TABLE tournaments ADD COLUMN terminal_kind TEXT
    CHECK (terminal_kind IS NULL OR terminal_kind IN (
        'completed', 'completed_with_failures', 'user_stopped', 'infrastructure_failure'
    ));
ALTER TABLE tournaments ADD COLUMN terminal_reason TEXT;
ALTER TABLE tournaments ADD COLUMN rating_status TEXT NOT NULL DEFAULT 'legacy_unknown'
    CHECK (rating_status IN (
        'legacy_unknown', 'provisional', 'rateable', 'insufficient_games',
        'disconnected_graph', 'estimator_failed'
    ));
ALTER TABLE tournaments ADD COLUMN rating_reason TEXT;

ALTER TABLE match_attempts ADD COLUMN failure_kind TEXT
    CHECK (failure_kind IS NULL OR failure_kind IN (
        'timeout', 'disconnected', 'spawn_failed', 'handshake_timeout',
        'protocol_error', 'cancelled', 'infrastructure', 'other'
    ));
ALTER TABLE match_attempts ADD COLUMN failure_phase TEXT
    CHECK (failure_phase IS NULL OR failure_phase IN (
        'setup', 'preprocessing', 'sync', 'move'
    ));
ALTER TABLE match_attempts ADD COLUMN failing_player_id TEXT;
";

const MIGRATIONS: &[(u32, &str)] = &[
    (1, MIGRATION_1),
    (2, MIGRATION_2),
    (3, MIGRATION_3),
    (4, MIGRATION_4),
    (5, MIGRATION_5),
    (6, MIGRATION_6),
    (7, MIGRATION_7),
    (8, MIGRATION_8),
];

pub fn initialize(conn: &mut Connection) -> Result<(), EvalError> {
    // PRAGMAs are per-connection. `foreign_keys` cannot be set inside a
    // transaction, so apply both before the migration loop opens any.
    //
    // `busy_timeout` makes a connection wait-and-retry (rather than fail with
    // SQLITE_BUSY) when another connection holds the write lock or is
    // checkpointing. WAL allows concurrent readers, but a reader can still
    // hit a momentary lock during a checkpoint — exactly the read-while-
    // writing the GUI relies on (list / standings while the runner writes).
    // 5s is generous for sub-second match writes and helps the CLI too.
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
    )?;

    let current: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for &(version, sql) in MIGRATIONS {
        if version > current {
            // Pre-flight checks must run before opening the transaction so a
            // refusal returns a typed error without a half-applied state.
            pre_migration_check(conn, version)?;

            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
            tx.commit()?;
        }
    }
    Ok(())
}

/// Per-migration sanity checks. Migration 3 drops and recreates the
/// tournament tables; refuse if any pre-migration tournaments exist,
/// since we cannot synthesize `game_config_id` or `tournament_seed`
/// for them.
fn pre_migration_check(conn: &Connection, version: u32) -> Result<(), EvalError> {
    if version == 3 {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM tournaments", [], |r| r.get(0))?;
        if count > 0 {
            return Err(EvalError::MigrationBlocked {
                version,
                message:
                    "tournaments table contains pre-migration rows; refusing to silently corrupt \
                     (no automatic backfill for game_config_id and tournament_seed)"
                        .into(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_8_preserves_v7_rows_as_explicitly_unknown() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        for &(version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version <= 7) {
            pre_migration_check(&conn, version).unwrap();
            conn.execute_batch(sql).unwrap();
            conn.execute_batch(&format!("PRAGMA user_version = {version}"))
                .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO game_configs (id, config_json) VALUES ('cfg', '{}');
             INSERT INTO players (id, display_name) VALUES ('a', 'A'), ('b', 'B');
             INSERT INTO tournaments
                (format, target_games_per_matchup, params_json, game_config_id, tournament_seed)
                VALUES ('round_robin', 2, '{}', 'cfg', 42);
             INSERT INTO tournament_players (tournament_id, player_id, slot)
                VALUES (1, 'a', 0), (1, 'b', 1);
             INSERT INTO match_attempts
                (tournament_id, game_config_id, player1_id, player2_id, seed,
                 repetition_index, attempt_index, status, failure_reason, finished_at)
                VALUES (1, 'cfg', 'a', 'b', 7, 0, 0, 'failure',
                        'timeout: move: Player1', '2026-08-12 10:00:00');",
        )
        .unwrap();

        initialize(&mut conn).unwrap();

        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 8);
        let tournament: (String, Option<String>, Option<String>, String) = conn
            .query_row(
                "SELECT lifecycle_status, terminal_kind, terminal_at, rating_status
                   FROM tournaments WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            tournament,
            ("legacy_unknown".into(), None, None, "legacy_unknown".into())
        );
        let failure: (String, Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT failure_reason, failure_kind, failure_phase, failing_player_id
                   FROM match_attempts WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(failure, ("timeout: move: Player1".into(), None, None, None));
    }
}
