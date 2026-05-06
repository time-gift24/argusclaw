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

export interface PendingAssistantSnapshotPayload {
  turn_number: number;
  content: string;
  reasoning: string;
  tool_calls: PendingToolCallSnapshotPayload[];
}

export interface PendingToolCallSnapshotPayload {
  index: number;
  call_id: string | null;
  name: string | null;
  arguments_text: string;
  status: "pending" | "started" | "completed";
  arguments: unknown | null;
  result: unknown | null;
  is_error: boolean;
}

export interface ThreadSnapshotPayload {
  session_id: string;
  thread_id: string;
  messages: ChatMessagePayload[];
  turn_count: number;
  token_count: number;
  plan_item_count: number;
  pending_assistant: PendingAssistantSnapshotPayload | null;
}

export type ThreadRuntimeStatus =
  | "inactive"
  | "loading"
  | "queued"
  | "running"
  | "cooling"
  | "evicted";

export interface ThreadPoolRuntimeSummary {
  thread_id: string;
  session_id: string | null;
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

export interface JobRuntimeSummary {
  thread_id: string;
  job_id: string;
  status: ThreadRuntimeStatus;
  estimated_memory_bytes: number;
  last_active_at: string | null;
  recoverable: boolean;
  last_reason: ThreadPoolEventReason | null;
}

export interface JobRuntimeSnapshot {
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

export interface JobRuntimeState {
  snapshot: JobRuntimeSnapshot;
  runtimes: JobRuntimeSummary[];
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

export type JobLifecycleStatus =
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

export interface JobStatusPayload {
  job_id: string;
  agent_id: number;
  prompt: string;
  status: JobLifecycleStatus;
  message?: string | null;
  agent_display_name?: string | null;
  agent_description?: string | null;
}

export type JobDetailStatus = JobLifecycleStatus;

export interface JobDetailTimelineItem {
  kind:
    | "dispatched"
    | "queued"
    | "started"
    | "cooling"
    | "evicted"
    | "result";
  at: string;
  label: string;
  status: JobDetailStatus;
  reason?: ThreadPoolEventReason | null;
}

export interface JobDetailPayload {
  job_id: string;
  agent_id: number;
  agent_display_name: string;
  agent_description: string | null;
  prompt: string;
  status: JobDetailStatus;
  summary_text: string | null;
  result_text: string | null;
  started_at: string | null;
  finished_at: string | null;
  input_tokens: number | null;
  output_tokens: number | null;
  source_message_id: string | null;
  thread_id: string | null;
  timeline: JobDetailTimelineItem[];
}

export interface MailboxMessageJobResultPayload {
  type: "job_result";
  job_id: string;
  success: boolean;
  cancelled: boolean;
  token_usage?: {
    input_tokens: number;
    output_tokens: number;
    total_tokens: number;
  } | null;
  agent_id: number;
  agent_display_name: string;
  agent_description: string;
}

export interface MailboxMessageTaskAssignmentPayload {
  type: "task_assignment";
  task_id: string;
  subject: string;
  description: string;
}

export interface MailboxMessagePlainPayload {
  type: "plain";
}

export type MailboxMessageTypePayload =
  | MailboxMessagePlainPayload
  | MailboxMessageJobResultPayload
  | MailboxMessageTaskAssignmentPayload;

export interface MailboxMessagePayload {
  id: string;
  from_thread_id: string;
  to_thread_id: string;
  from_label: string;
  message_type: MailboxMessageTypePayload;
  text: string;
  timestamp: string;
  read: boolean;
  summary?: string | null;
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
  | {
      type: "thread_pool_queued";
      session_id: string | null;
    }
  | {
      type: "thread_pool_started";
      session_id: string | null;
    }
  | {
      type: "thread_pool_cooling";
      session_id: string | null;
    }
  | {
      type: "thread_pool_evicted";
      session_id: string | null;
      reason: ThreadPoolEventReason;
    }
  | {
      type: "thread_pool_metrics_updated";
      snapshot: ThreadPoolSnapshot;
    }
  | {
      type: "job_runtime_queued";
      job_id: string;
    }
  | {
      type: "job_runtime_started";
      job_id: string;
    }
  | {
      type: "job_runtime_cooling";
      job_id: string;
    }
  | {
      type: "job_runtime_evicted";
      job_id: string;
      reason: ThreadPoolEventReason;
    }
  | {
      type: "job_runtime_updated";
      runtime: JobRuntimeSummary;
    }
  | {
      type: "job_runtime_metrics_updated";
      snapshot: JobRuntimeSnapshot;
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
      cancelled: boolean;
      message: string;
      input_tokens?: number | null;
      output_tokens?: number | null;
      agent_id: number;
      agent_display_name: string;
      agent_description: string;
    }
  | {
      type: "mailbox_message_queued";
      message: MailboxMessagePayload;
    };

export interface ThreadEventEnvelope {
  session_id: string;
  thread_id: string;
  turn_number?: number | null;
  payload: ThreadEventPayload;
}
