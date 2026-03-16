import { invoke } from "@tauri-apps/api/core";

// Types matching Rust structs

export interface LlmProviderSummary {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  model: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
}

export interface LlmProviderRecord {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  model: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
}

export interface ProviderInput {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  model: string;
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
  id: string;
  display_name: string;
  description: string;
  version: string;
  provider_id: string;
  system_prompt: string;
  tool_names: string[];
  max_tokens?: number;
  temperature?: number;
}

// LLMProvider API
export const providers = {
  list: () => invoke<LlmProviderSummary[]>("list_providers"),

  get: (id: string) => invoke<LlmProviderRecord | null>("get_provider", { id }),

  upsert: (record: ProviderInput) =>
    invoke<void>("upsert_provider", { record }),

  delete: (id: string) => invoke<boolean>("delete_provider", { id }),

  setDefault: (id: string) => invoke<void>("set_default_provider", { id }),

  testConnection: (id: string) =>
    invoke<ProviderTestResult>("test_provider_connection", { id }),
};

// Agent API
export const agents = {
  list: () => invoke<AgentRecord[]>("list_agent_templates"),

  get: (id: string) => invoke<AgentRecord | null>("get_agent_template", { id }),

  upsert: (record: AgentRecord) =>
    invoke<void>("upsert_agent_template", { record }),

  delete: (id: string) => invoke<boolean>("delete_agent_template", { id }),
};
