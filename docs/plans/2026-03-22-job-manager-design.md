# JobManager & Subagent Dispatch Design

## Overview

Enable agent templates to dispatch background jobs to subagent templates via a `dispatch_job` tool call. The job executes asynchronously with completion notification via SSE + polling fallback.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        argus-session                         │
│  ┌─────────────┐    ┌─────────────────────────────────┐    │
│  │ TurnManager │───►│  JobManager (new argus-job crate)│    │
│  └─────────────┘    │  - JobRegistry                   │    │
│                     │  - SSEBroadcaster (session-scoped)│    │
│                     │  - JobExecution (reuses turn)    │    │
│                     └─────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

### argus-job Crate

New crate `argus-job` that:
- Depends on `argus-protocol`, `argus-turn`, `argus-llm`, `argus-tool`
- Reuses `argus-turn` execution patterns (Turn → TurnExecution → TurnDelegate)
- Implements `JobManager` with SSE-based completion notification

## Database Schema

### Agents Table Changes

```sql
ALTER TABLE agents ADD COLUMN parent_agent_id INTEGER REFERENCES agents(id);
ALTER TABLE agents ADD COLUMN agent_type TEXT DEFAULT 'standard' CHECK(agent_type IN ('standard', 'subagent'));
```

- `parent_agent_id`: Links subagent to its parent agent (NULL = main agent)
- `agent_type`: `'standard'` (can dispatch jobs) or `'subagent'` (cannot dispatch)

### Jobs Table Changes

```sql
ALTER TABLE jobs ADD COLUMN parent_job_id TEXT REFERENCES jobs(id);
```

- `parent_job_id`: Links child job to parent job for hierarchy tracking

## Tool Interface

### dispatch_job Tool

```rust
dispatch_job(
    prompt: String,           // Job prompt
    agent_id: i64,            // Subagent template ID to use
    context: Option<JSON>,    // Additional context for job
    wait_for_result: bool,   // If true, block until complete
) -> Result<JobDispatchResult, DispatchError>
```

```rust
struct JobDispatchResult {
    job_id: WorkflowId,
    status: String,          // "submitted" or "completed"
    result: Option<JobResult>, // Populated if wait_for_result=true
}
```

### Error Handling

- **Permission denied**: If `agent_type == 'subagent'`, return error
- **Invalid agent**: If agent_id doesn't exist, return error
- **Retry with backoff**: On transient failures (rate limit, etc), retry up to 3 times

## Completion Detection

### Primary: SSE Event

When job completes, broadcast SSE event:
```rust
SseEvent::JobResult {
    job_id: String,
    status: String,        // "completed", "failed", "stuck"
    session_id: Option<String>,
}
```

### Fallback: Polling

Agent can poll `get_job_result(job_id)` to check status and retrieve result.

## Frontend UI

Nested UI under agent settings page:
- Parent agent card shows "Subagents" section
- List subagents with add/remove capability
- Add subagent: Select from existing agents or create new
- Remove: Disassociate subagent (does not delete agent record)

## Implementation Phases

### Phase 1: Database & Types
- Add columns to agents table (migration)
- Add `parent_job_id` to jobs table (migration)
- Add `parent_agent_id`, `agent_type` to AgentRecord
- Add JobRecord.parent_job_id field

### Phase 2: argus-job Crate
- Create crate structure
- Implement JobManager
- Implement SSEBroadcaster (session-scoped)
- Adapt turn execution patterns for job execution
- Implement dispatch_job tool

### Phase 3: Frontend
- Add subagent management UI
- Agent list shows nested subagents
- Add/remove subagent functionality

### Phase 4: Integration & Constraints
- Subagent cannot dispatch jobs (agent_type check)
- Job completion SSE → TurnManager notification
- Polling fallback implementation
