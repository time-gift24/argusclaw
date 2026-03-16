export interface ToolCallPayload {
  id: string;
  name: string;
  arguments: unknown;
}

export interface ChatMessagePayload {
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  tool_call_id?: string | null;
  name?: string | null;
  tool_calls?: ToolCallPayload[] | null;
}

export interface ThreadSnapshotPayload {
  runtime_agent_id: string;
  thread_id: string;
  messages: ChatMessagePayload[];
  turn_count: number;
  token_count: number;
}

export interface ChatSessionPayload {
  session_key: string;
  template_id: string;
  runtime_agent_id: string;
  thread_id: string;
  effective_provider_id: string;
}

export type ThreadEventPayload =
  | { type: "ReasoningDelta"; delta: string }
  | { type: "ContentDelta"; delta: string }
  | {
      type: "ToolCallDelta";
      index: number;
      id?: string | null;
      name?: string | null;
      arguments_delta?: string | null;
    }
  | { type: "LlmUsage"; input_tokens: number; output_tokens: number }
  | { type: "ToolStarted"; tool_call_id: string; tool_name: string; arguments: unknown }
  | { type: "ToolCompleted"; tool_call_id: string; tool_name: string; result: unknown }
  | { type: "TurnCompleted"; input_tokens: number; output_tokens: number; total_tokens: number }
  | { type: "TurnFailed"; error: string }
  | { type: "Idle" }
  | { type: "Compacted"; new_token_count: number }
  | { type: "WaitingForApproval"; request: Record<string, unknown> }
  | { type: "ApprovalResolved"; response: Record<string, unknown> };

export interface ThreadEventEnvelope {
  runtime_agent_id: string;
  thread_id: string;
  turn_number?: number | null;
  payload: ThreadEventPayload;
}

export type ApprovalDecision = "approved" | "denied" | "timed_out";
