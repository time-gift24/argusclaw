import { invoke } from "@tauri-apps/api/core";

// Types matching Rust structs

export type ProviderSecretStatus = "ready" | "requires_reentry";

export interface LlmProviderSummary {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}

export interface LlmProviderRecord {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}

export interface ProviderInput {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
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
}

// Model Types
export interface LlmModelRecord {
  id: string;
  provider_id: string;
  name: string;
  is_default: boolean;
}

export interface ModelInput {
  id: string;
  provider_id: string;
  name: string;
  is_default: boolean;
}

export interface AgentRecord {
  id: string;
  display_name: string;
  description: string;
  version: string;
  provider_id: string;
  system_prompt: string;
  tool_names: string[];
  max_tokens?: number;
  temperature?: number;
  model_id?: string;
}

// Provider API
export const providers = {
  list: () => invoke<LlmProviderSummary[]>("list_providers"),

  get: (id: string) => invoke<LlmProviderRecord | null>("get_provider", { id }),

  upsert: (record: ProviderInput) =>
    invoke<void>("upsert_provider", { record }),

  delete: (id: string) => invoke<boolean>("delete_provider", { id }),

  setDefault: (id: string) => invoke<void>("set_default_provider", { id }),

  testConnection: (id: string) =>
    invoke<ProviderTestResult>("test_provider_connection", { id }),

  testInput: (record: ProviderInput, modelName: string) =>
    invoke<ProviderTestResult>("test_provider_input", { record, modelName }),
};

// Model API
export const models = {
  listByProvider: (providerId: string) =>
    invoke<LlmModelRecord[]>("list_models_by_provider", { providerId }),

  upsert: (record: ModelInput) =>
    invoke<void>("upsert_model", { record }),

  delete: (id: string) => invoke<boolean>("delete_model", { id }),

  setDefault: (id: string) => invoke<void>("set_default_model", { id }),
};

// Tools API
export const tools = {
  listBuiltin: () => invoke<string[]>("list_builtin_tools"),
};

// Agent API
export const agents = {
  list: () => invoke<AgentRecord[]>("list_agent_templates"),

  get: (id: string) => invoke<AgentRecord | null>("get_agent_template", { id }),

  upsert: (record: AgentRecord) =>
    invoke<void>("upsert_agent_template", { record }),

  delete: (id: string) => invoke<boolean>("delete_agent_template", { id }),
};

// Chat API
export interface ChatSessionPayload {
  session_key: string;
  template_id: string;
  runtime_agent_id: string;
  thread_id: string;
  effective_provider_id: string;
}

export interface ThreadSnapshotPayload {
  runtime_agent_id: string;
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
  createChatSession: (templateId: string, providerPreferenceId: string | null) =>
    invoke<ChatSessionPayload>("create_chat_session", {
      templateId,
      providerPreferenceId,
    }),

  sendMessage: (runtimeAgentId: string, threadId: string, content: string) =>
    invoke<void>("send_message", { runtimeAgentId, threadId, content }),

  getThreadSnapshot: (runtimeAgentId: string, threadId: string) =>
    invoke<ThreadSnapshotPayload>("get_thread_snapshot", {
      runtimeAgentId,
      threadId,
    }),

  resolveApproval: (
    runtimeAgentId: string,
    requestId: string,
    decision: ApprovalDecision,
    resolvedBy?: string | null,
  ) =>
    invoke<void>("resolve_approval", {
      runtimeAgentId,
      requestId,
      decision,
      resolvedBy,
    }),
};
