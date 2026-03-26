export interface ToolCallPayload {
  id: string;
  name: string;
  arguments: unknown;
}

export interface ApprovalRequestPayload {
  id: string;
  agent_id: string;
  tool_name: string;
  action: string;
  risk_level: "low" | "medium" | "high" | "critical";
  requested_at: string;
  timeout_secs: number;
}

export interface ApprovalResponsePayload {
  request_id: string;
  decision: ApprovalDecision;
  decided_at: string;
  decided_by?: string | null;
}

export interface ChatMessagePayload {
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  reasoning_content?: string | null;
  tool_call_id?: string | null;
  name?: string | null;
  tool_calls?: ToolCallPayload[] | null;
}

export interface ThreadSnapshotPayload {
  session_id: string;
  thread_id: string;
  messages: ChatMessagePayload[];
  turn_count: number;
  token_count: number;
  plan_item_count: number;
}

export interface ChatSessionPayload {
  session_key: string;
  template_id: number;
  session_id: string;
  thread_id: string;
  effective_provider_id: number | null;
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
      type: "tool_call_delta";
      index: number;
      id?: string | null;
      name?: string | null;
      arguments_delta?: string | null;
    }
  | { type: "llm_usage"; input_tokens: number; output_tokens: number }
  | { type: "tool_started"; tool_call_id: string; tool_name: string; arguments: unknown }
  | {
      type: "tool_completed";
      tool_call_id: string;
      tool_name: string;
      result: unknown;
      is_error: boolean;
    }
  | { type: "turn_completed"; input_tokens: number; output_tokens: number; total_tokens: number }
  | { type: "turn_failed"; error: string }
  | { type: "idle" }
  | { type: "compacted"; new_token_count: number }
  | { type: "job_dispatched"; job_id: string; agent_id: number; prompt: string; context?: unknown | null }
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
    }
  | { type: "waiting_for_approval"; request: ApprovalRequestPayload }
  | { type: "approval_resolved"; response: ApprovalResponsePayload };

export interface ThreadEventEnvelope {
  session_id: string;
  thread_id: string;
  turn_number?: number | null;
  payload: ThreadEventPayload;
}

export type ApprovalDecision = "approved" | "denied" | "timed_out";
