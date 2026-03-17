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

export interface LlmProviderRecord {
  id: number;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  models: string[];
  default_model: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}

export interface ProviderInput {
  id: number;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  models: string[];
  default_model: string;
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

export interface AgentRecord {
  id: number;
  display_name: string;
  description: string;
  version: string;
  provider_id: number | null;
  system_prompt: string;
  tool_names: string[];
  max_tokens?: number;
  temperature?: number;
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
};

// Agent API
export const agents = {
  list: () => invoke<AgentRecord[]>("list_agent_templates"),

  get: (id: number) => invoke<AgentRecord | null>("get_agent_template", { id }),

  upsert: (record: AgentRecord) =>
    invoke<void>("upsert_agent_template", {
      record: {
        ...record,
        provider_id: record.provider_id != null ? Number(record.provider_id) : null,
      },
    }),

  delete: (id: number) => invoke<boolean>("delete_agent_template", { id }),
};

// Chat API
export interface ChatSessionPayload {
  session_key: string;
  template_id: number;
  runtime_agent_id: number;
  thread_id: string;
  effective_provider_id: number;
  effective_model: string;
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
  createChatSession: (
    templateId: number,
    providerPreferenceId: number | null,
    modelOverride: string | null,
  ) =>
    invoke<ChatSessionPayload>("create_chat_session", {
      templateId: templateId.toString(),
      providerPreferenceId: providerPreferenceId?.toString() ?? null,
      modelOverride,
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
