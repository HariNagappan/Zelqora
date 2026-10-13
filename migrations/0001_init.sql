-- Nova initial schema (migration 0001)
-- Applied by the embedded migration runner in src-tauri/src/database/mod.rs
-- using SQLite's `PRAGMA user_version`.

CREATE TABLE IF NOT EXISTS todos (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT    NOT NULL,
    description TEXT    NOT NULL DEFAULT '',
    completed   INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL,
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_todos_completed ON todos (completed);
CREATE INDEX IF NOT EXISTS idx_todos_created   ON todos (created_at DESC);

CREATE TABLE IF NOT EXISTS activity_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT    NOT NULL,
    transcript  TEXT    NOT NULL DEFAULT '',
    intent      TEXT,
    parameters  TEXT,
    result      TEXT,
    success     INTEGER NOT NULL DEFAULT 0,
    response    TEXT    NOT NULL DEFAULT '',
    duration_ms INTEGER
);

CREATE INDEX IF NOT EXISTS idx_activity_timestamp ON activity_log (timestamp DESC);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
