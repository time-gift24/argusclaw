-- Add parent_job_id column to jobs table
-- Links child job to parent job for hierarchy tracking

ALTER TABLE jobs ADD COLUMN parent_job_id TEXT REFERENCES jobs(id);

-- Index for efficient parent job lookup
CREATE INDEX IF NOT EXISTS idx_jobs_parent_job_id ON jobs(parent_job_id);
