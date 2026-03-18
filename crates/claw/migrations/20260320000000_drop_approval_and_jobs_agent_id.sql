-- Drop approval_requests and approval_responses tables
-- These are ephemeral state that should not be persisted
DROP TABLE IF EXISTS approval_responses;
DROP TABLE IF EXISTS approval_requests;

-- Note: jobs.agent_id is kept - needed for cron jobs with thread_id=None
-- to determine which agent to run
