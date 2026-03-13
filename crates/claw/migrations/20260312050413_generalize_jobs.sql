-- Drop old stage-dependent tables and recreate jobs as universal
DROP TABLE IF EXISTS jobs;
DROP TABLE IF EXISTS stages;

CREATE TABLE IF NOT EXISTS jobs (
    id          TEXT PRIMARY KEY NOT NULL,
    job_type    TEXT NOT NULL DEFAULT 'standalone',
    name        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',

    agent_id    TEXT NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    context     TEXT,
    prompt      TEXT NOT NULL,
    thread_id   TEXT,

    group_id    TEXT,
    depends_on  TEXT NOT NULL DEFAULT '[]',

    cron_expr   TEXT,
    scheduled_at TEXT,

    started_at  TEXT,
    finished_at TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
CREATE INDEX IF NOT EXISTS idx_jobs_group_id ON jobs(group_id);
CREATE INDEX IF NOT EXISTS idx_jobs_agent_id ON jobs(agent_id);
CREATE INDEX IF NOT EXISTS idx_jobs_scheduled_at ON jobs(scheduled_at);
CREATE INDEX IF NOT EXISTS idx_jobs_job_type ON jobs(job_type);
