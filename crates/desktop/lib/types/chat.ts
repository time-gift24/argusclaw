export interface ToolCallPayload {
  id: string;
  name: string;
  arguments: unknown;
}

export interface ChatMessageMetadataPayload {
  summary: boolean;
  mode?:
    | "compaction_prompt"
    | "compaction_summary"
    | "compaction_replay"
    | null;
  synthetic: boolean;
  collapsed_by_default: boolean;
}

export interface ChatMessagePayload {
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  reasoning_content?: string | null;
  tool_call_id?: string | null;
  name?: string | null;
  tool_calls?: ToolCallPayload[] | null;
  metadata?: ChatMessageMetadataPayload | null;
}

export interface ThreadSnapshotPayload {
  session_id: string;
  thread_id: string;
  messages: ChatMessagePayload[];
  turn_count: number;
  token_count: number;
  plan_item_count: number;
}

export type ThreadRuntimeStatus =
  | "inactive"
  | "loading"
  | "queued"
  | "running"
  | "cooling"
  | "evicted";

export type ThreadPoolRuntimeKind = "chat" | "job";

export interface ThreadPoolRuntimeRef {
  thread_id: string;
  kind: ThreadPoolRuntimeKind;
  session_id: string | null;
  job_id: string | null;
}

export interface ThreadPoolRuntimeSummary {
  runtime: ThreadPoolRuntimeRef;
  status: ThreadRuntimeStatus;
  estimated_memory_bytes: number;
  last_active_at: string | null;
  recoverable: boolean;
  last_reason: ThreadPoolEventReason | null;
}

export interface ThreadPoolSnapshot {
  max_threads: number;
  active_threads: number;
  queued_threads: number;
  running_threads: number;
  cooling_threads: number;
  evicted_threads: number;
  estimated_memory_bytes: number;
  peak_estimated_memory_bytes: number;
  process_memory_bytes: number | null;
  peak_process_memory_bytes: number | null;
  resident_thread_count: number;
  avg_thread_memory_bytes: number;
  captured_at: string;
}

export interface ThreadPoolState {
  snapshot: ThreadPoolSnapshot;
  runtimes: ThreadPoolRuntimeSummary[];
}

export type ThreadPoolEventReason =
  | "cooling_expired"
  | "memory_pressure"
  | "cancelled"
  | "execution_failed";

export interface ChatSessionPayload {
  session_key: string;
  template_id: number;
  session_id: string;
  thread_id: string;
  effective_provider_id: number | null;
  effective_model: string | null;
}

export interface JobStatusPayload {
  job_id: string;
  agent_id: number;
  prompt: string;
  status: "running" | "completed" | "failed";
  message?: string | null;
  agent_display_name?: string | null;
  agent_description?: string | null;
}

export type ThreadEventPayload =
  | { type: "reasoning_delta"; delta: string }
  | { type: "content_delta"; delta: string }
  | {
      type: "retry_attempt";
      attempt: number;
      max_retries: number;
      error: string;
    }
  | {
      type: "tool_call_delta";
      index: number;
      id?: string | null;
      name?: string | null;
      arguments_delta?: string | null;
    }
  | {
      type: "llm_usage";
      input_tokens: number;
      output_tokens: number;
      total_tokens: number;
    }
  | {
      type: "tool_started";
      tool_call_id: string;
      tool_name: string;
      arguments: unknown;
    }
  | {
      type: "tool_completed";
      tool_call_id: string;
      tool_name: string;
      result: unknown;
      is_error: boolean;
    }
  | {
      type: "turn_completed";
      input_tokens: number;
      output_tokens: number;
      total_tokens: number;
    }
  | { type: "turn_failed"; error: string }
  | { type: "idle" }
  | { type: "compacted"; new_token_count: number }
  | { type: "compaction_started" }
  | { type: "compaction_finished" }
  | { type: "compaction_failed"; error: string }
  | { type: "thread_bound_to_job"; job_id: string }
  | { type: "thread_pool_queued"; runtime: ThreadPoolRuntimeRef }
  | { type: "thread_pool_started"; runtime: ThreadPoolRuntimeRef }
  | { type: "thread_pool_cooling"; runtime: ThreadPoolRuntimeRef }
  | {
      type: "thread_pool_evicted";
      runtime: ThreadPoolRuntimeRef;
      reason: ThreadPoolEventReason;
    }
  | {
      type: "thread_pool_metrics_updated";
      snapshot: ThreadPoolSnapshot;
    }
  | {
      type: "job_dispatched";
      job_id: string;
      agent_id: number;
      prompt: string;
      context?: unknown | null;
    }
  | {
      type: "job_result";
      job_id: string;
      success: boolean;
      message: string;
      input_tokens?: number | null;
      output_tokens?: number | null;
      agent_id: number;
      agent_display_name: string;
      agent_description: string;
    };

export interface ThreadEventEnvelope {
  session_id: string;
  thread_id: string;
  turn_number?: number | null;
  payload: ThreadEventPayload;
}
