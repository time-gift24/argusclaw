-- Drop approval_requests and approval_responses tables
-- These are ephemeral state that should not be persisted
DROP TABLE IF EXISTS approval_responses;
DROP TABLE IF EXISTS approval_requests;

-- Remove agent_id column from jobs (冗余：可通过 thread.template_id 获取 agent)
-- SQLite 不支持 DROP COLUMN，需重建表
CREATE TABLE jobs_new AS SELECT
    id, job_type, name, status, context, prompt, thread_id,
    group_id, depends_on, cron_expr, scheduled_at,
    started_at, finished_at, created_at, updated_at
FROM jobs;

DROP TABLE jobs;
ALTER TABLE jobs_new RENAME TO jobs;

-- 重建索引
CREATE INDEX idx_jobs_status ON jobs(status);
CREATE INDEX idx_jobs_group_id ON jobs(group_id);
CREATE INDEX idx_jobs_scheduled_at ON jobs(scheduled_at);
CREATE INDEX idx_jobs_job_type ON jobs(job_type);
