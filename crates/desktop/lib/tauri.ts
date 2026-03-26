import { invoke } from "@tauri-apps/api/core";

// Types matching Rust structs

export type ProviderSecretStatus = "ready" | "requires_reentry";

export interface LlmProviderSummary {
  id: number;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  models: string[];
  default_model: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}

export interface ModelConfig {
  max_context_window: number;
}

export interface LlmProviderRecord {
  id: number;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  models: string[];
  model_config: Record<string, ModelConfig>;
  default_model: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
  meta_data: Record<string, string>;
}
export interface ProviderInput {
  id: number;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  models: string[];
  model_config: Record<string, ModelConfig>;
  default_model: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
  meta_data: Record<string, string>;
}

export type ProviderTestStatus =
  | "success"
  | "auth_failed"
  | "model_not_available"
  | "rate_limited"
  | "request_failed"
  | "invalid_response"
  | "provider_not_found"
  | "unsupported_provider_kind";

export interface ProviderTestResult {
  provider_id: string;
  model: string;
  base_url: string;
  checked_at: string;
  latency_ms: number;
  status: ProviderTestStatus;
  message: string;
  request?: string;
  response?: string;
}

export interface ToolInfo {
  name: string;
  description: string;
  risk_level: "low" | "medium" | "high" | "critical";
  parameters: Record<string, unknown>;
}

export interface ThinkingConfig {
  type: "enabled" | "disabled";
  clear_thinking: boolean;
}

export interface AgentRecord {
  id: number;
  display_name: string;
  description: string;
  version: string;
  provider_id: number | null;
  model_id?: string | null;
  system_prompt: string;
  tool_names: string[];
  parent_agent_id?: number;
  agent_type?: "standard" | "subagent";
  max_tokens?: number;
  temperature?: number;
  thinking_config?: ThinkingConfig;
}

// LLMProvider API
export const providers = {
  list: () => invoke<LlmProviderSummary[]>("list_providers"),

  get: (id: number) => invoke<LlmProviderRecord | null>("get_provider", { id }),

  upsert: (record: ProviderInput) =>
    invoke<string>("upsert_provider", { record }).then((id) => parseInt(id, 10)),

  delete: (id: number) => invoke<boolean>("delete_provider", { id }),

  setDefault: (id: number) => invoke<void>("set_default_provider", { id }),

  testConnection: (id: number, model: string) =>
    invoke<ProviderTestResult>("test_provider_connection", { id, model }),

  testInput: (record: ProviderInput, model: string) =>
    invoke<ProviderTestResult>("test_provider_input", { record, model }),

  getContextWindow: (providerId: number) =>
    invoke<number>("get_provider_context_window", { providerId }),
};

// Agent API
export const agents = {
  list: () => invoke<AgentRecord[]>("list_agent_templates"),

  get: (id: number) => invoke<AgentRecord | null>("get_agent_template", { id }),

  upsert: (record: AgentRecord) =>
    invoke<string>("upsert_agent_template", {
      record: {
        ...record,
        provider_id: record.provider_id != null ? Number(record.provider_id) : null,
      },
    }).then((id) => parseInt(id, 10)),

  delete: (id: number) => invoke<boolean>("delete_agent_template", { id }),

  listSubagents: (parentId: number) =>
    invoke<AgentRecord[]>("list_subagents", { parentId }),

  addSubagent: (parentId: number, childId: number) =>
    invoke<void>("add_subagent", { parentId, childId }),

  removeSubagent: (parentId: number, childId: number) =>
    invoke<void>("remove_subagent", { parentId, childId }),
};

// Tools API
export const tools = {
  list: () => invoke<ToolInfo[]>("list_tools"),
};

// Session API
export interface SessionSummary {
  id: string;
  name: string;
  thread_count: number;
  updated_at: string;
}

export interface ThreadSummary {
  thread_id: string;
  title: string | null;
  turn_count: number;
  token_count: number;
  updated_at: string;
}

export const sessions = {
  list: () => invoke<SessionSummary[]>("list_sessions"),

  delete: (sessionId: string) => invoke<void>("delete_session", { sessionId }),

  renameSession: (sessionId: string, name: string) =>
    invoke<void>("rename_session", { sessionId, name }),

  renameThread: (sessionId: string, threadId: string, title: string) =>
    invoke<void>("rename_thread", { sessionId, threadId, title }),
};

// Chat API
export interface ChatSessionPayload {
  session_key: string;
  session_id: string;
  template_id: number;
  thread_id: string;
  effective_provider_id: number | null;
  effective_model: string | null;
}

export interface ThreadSnapshotPayload {
  session_id: string;
  thread_id: string;
  messages: Array<{
    role: "system" | "user" | "assistant" | "tool";
    content: string;
    reasoning_content?: string | null;
    tool_call_id?: string | null;
    name?: string | null;
    tool_calls?: Array<{ id: string; name: string; arguments: unknown }> | null;
  }>;
  turn_count: number;
  token_count: number;
}

export type ApprovalDecision = "approved" | "denied" | "timed_out";

export const chat = {
  createChatSession: (
    templateId: number,
    providerPreferenceId: number | null,
    model: string | null,
  ) =>
    invoke<ChatSessionPayload>("create_chat_session", {
      templateId: templateId.toString(),
      providerPreferenceId: providerPreferenceId?.toString() ?? null,
      model,
    }),

  activateExistingThread: (sessionId: string, threadId: string) =>
    invoke<ChatSessionPayload>("activate_existing_thread", {
      sessionId,
      threadId,
    }),

  sendMessage: (sessionId: string, threadId: string, content: string) =>
    invoke<void>("send_message", { sessionId, threadId, content }),

  getThreadSnapshot: (sessionId: string, threadId: string) =>
    invoke<ThreadSnapshotPayload>("get_thread_snapshot", {
      sessionId,
      threadId,
    }),

  listThreads: (sessionId: string) =>
    invoke<ThreadSummary[]>("list_threads", { sessionId }),

  resolveApproval: (
    requestId: string,
    decision: ApprovalDecision,
    resolvedBy?: string | null,
  ) =>
    invoke<void>("resolve_approval", {
      requestId,
      decision,
      resolvedBy,
    }),
};
