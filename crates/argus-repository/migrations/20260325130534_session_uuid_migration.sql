-- Migrate sessions.id and threads.session_id from INTEGER to TEXT (UUID).
-- This supports UUIDv7-based SessionId with time-sortable IDs.
--
-- Strategy: recreate tables with TEXT columns first, then populate them with
-- converted data. This avoids SQLite datatype mismatch during in-place UPDATE.

CREATE TABLE IF NOT EXISTS sessions_new (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO sessions_new (id, name, created_at, updated_at)
SELECT
    printf('00000000-0000-7000-8000-00000000%04X', id),
    name,
    created_at,
    updated_at
FROM sessions;

DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;

CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC);

CREATE TABLE IF NOT EXISTS threads_new (
    id TEXT PRIMARY KEY,
    provider_id INTEGER NOT NULL REFERENCES llm_providers(id) ON DELETE RESTRICT,
    title TEXT,
    token_count INTEGER NOT NULL DEFAULT 0,
    turn_count INTEGER NOT NULL DEFAULT 0,
    session_id TEXT REFERENCES sessions(id),
    template_id INTEGER REFERENCES agents(id),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO threads_new (
    id,
    provider_id,
    title,
    token_count,
    turn_count,
    session_id,
    template_id,
    created_at,
    updated_at
)
SELECT
    id,
    provider_id,
    title,
    token_count,
    turn_count,
    CASE
        WHEN session_id IS NOT NULL THEN printf('00000000-0000-7000-8000-00000000%04X', session_id)
        ELSE NULL
    END,
    template_id,
    created_at,
    updated_at
FROM threads;

DROP TABLE threads;
ALTER TABLE threads_new RENAME TO threads;

CREATE INDEX IF NOT EXISTS idx_threads_provider_id ON threads(provider_id);
CREATE INDEX IF NOT EXISTS idx_threads_updated_at ON threads(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_threads_session_id ON threads(session_id);
