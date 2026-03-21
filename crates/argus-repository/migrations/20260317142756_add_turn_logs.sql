-- Add session_id and template_id to threads (nullable initially)
ALTER TABLE threads ADD COLUMN session_id INTEGER REFERENCES sessions(id);
ALTER TABLE threads ADD COLUMN template_id INTEGER REFERENCES agents(id);

-- Update existing threads to belong to Legacy session and default template
UPDATE threads SET session_id = 1 WHERE session_id IS NULL;
UPDATE threads SET template_id = (SELECT id FROM agents ORDER BY id LIMIT 1) WHERE template_id IS NULL;

-- Add indexes for new columns
CREATE INDEX IF NOT EXISTS idx_threads_session_id ON threads(session_id);
