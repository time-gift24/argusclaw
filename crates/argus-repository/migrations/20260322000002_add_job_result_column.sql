-- Add result column to jobs table for storing job execution results
-- Stores JSON: { "success": bool, "message": string, "token_usage": { "input_tokens": u32, "output_tokens": u32, "total_tokens": u32 } | null }

ALTER TABLE jobs ADD COLUMN result TEXT;

-- Index for querying jobs by success/failure status
CREATE INDEX IF NOT EXISTS idx_jobs_result ON jobs(result);
