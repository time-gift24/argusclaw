-- Create turn_logs table
CREATE TABLE IF NOT EXISTS turn_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    turn_seq INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    model TEXT NOT NULL,
    latency_ms INTEGER NOT NULL,
    turn_data TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(thread_id, turn_seq)
);

CREATE INDEX IF NOT EXISTS idx_turn_logs_thread ON turn_logs(thread_id);
CREATE INDEX IF NOT EXISTS idx_turn_logs_created ON turn_logs(created_at);

-- Add session_id and template_id to threads (nullable initially)
ALTER TABLE threads ADD COLUMN session_id INTEGER REFERENCES sessions(id);
ALTER TABLE threads ADD COLUMN template_id INTEGER REFERENCES agents(id);

-- Update existing threads to belong to Legacy session and default template
UPDATE threads SET session_id = 1 WHERE session_id IS NULL;
UPDATE threads SET template_id = (SELECT id FROM agents ORDER BY id LIMIT 1) WHERE template_id IS NULL;

-- Add indexes for new columns
CREATE INDEX IF NOT EXISTS idx_threads_session_id ON threads(session_id);
