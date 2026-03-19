-- Add checkpoint and history persistence features
-- Migration: 20260319120000_add_checkpoint_features.sql

-- Add turn_seq to messages table to link messages to turns
ALTER TABLE messages ADD COLUMN turn_seq INTEGER NOT NULL DEFAULT 0;

-- Add status field to turn_logs (completed | rolled_back)
ALTER TABLE turn_logs ADD COLUMN status TEXT NOT NULL DEFAULT 'completed';

-- Add count fields for quick queries
ALTER TABLE turn_logs ADD COLUMN tool_calls_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE turn_logs ADD COLUMN messages_count INTEGER NOT NULL DEFAULT 0;

-- Create unique constraint on turn_logs (thread_id, turn_seq)
-- Note: UNIQUE constraint already exists, so we skip this

-- Create indexes for query optimization
CREATE INDEX IF NOT EXISTS idx_messages_thread_turn ON messages(thread_id, turn_seq);
CREATE INDEX IF NOT EXISTS idx_messages_thread_seq ON messages(thread_id, seq);
CREATE INDEX IF NOT EXISTS idx_messages_thread_seq_desc ON messages(thread_id, seq DESC);
CREATE INDEX IF NOT EXISTS idx_turn_logs_thread_status ON turn_logs(thread_id, status);
CREATE INDEX IF NOT EXISTS idx_turn_logs_thread_turn_desc ON turn_logs(thread_id, turn_seq DESC);
