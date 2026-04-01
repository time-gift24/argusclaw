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
    invoke<string>("upsert_provider", { record }).then((id) =>
      parseInt(id, 10),
    ),

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
        provider_id:
          record.provider_id != null ? Number(record.provider_id) : null,
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

// Knowledge API
export interface KnowledgeRepoRecord {
  id: number;
  repo: string;
  repo_id: string;
  provider: string;
  owner: string;
  name: string;
  default_branch: string;
  manifest_paths: string[];
  workspace: string;
}

export const knowledge = {
  list: () => invoke<KnowledgeRepoRecord[]>("list_knowledge_repos"),

  upsert: (record: KnowledgeRepoRecord) =>
    invoke<number>("upsert_knowledge_repo", { record }),

  delete: (id: number) => invoke<boolean>("delete_knowledge_repo", { id }),

  listAgentWorkspaces: (agentId: number) =>
    invoke<string[]>("list_agent_knowledge_workspaces", { agentId }),

  setAgentWorkspaces: (agentId: number, workspaces: string[]) =>
    invoke<void>("set_agent_knowledge_workspaces", { agentId, workspaces }),
};

// MCP API
export type McpServerStatus =
  | "ready"
  | "connecting"
  | "retrying"
  | "failed"
  | "disabled";

export type McpTransportConfig =
  | {
      kind: "stdio";
      command: string;
      args: string[];
      env: Record<string, string>;
    }
  | {
      kind: "http";
      url: string;
      headers: Record<string, string>;
    }
  | {
      kind: "sse";
      url: string;
      headers: Record<string, string>;
    };

export interface McpServerRecord {
  id: number | null;
  display_name: string;
  enabled: boolean;
  transport: McpTransportConfig;
  timeout_ms: number;
  status: McpServerStatus;
  last_checked_at: string | null;
  last_success_at: string | null;
  last_error: string | null;
  discovered_tool_count: number;
}

export interface McpDiscoveredToolRecord {
  server_id: number;
  tool_name_original: string;
  description: string;
  schema: Record<string, unknown>;
  annotations: Record<string, unknown> | null;
}

export interface McpConnectionTestResult {
  status: McpServerStatus;
  checked_at: string;
  latency_ms: number;
  discovered_tools: McpDiscoveredToolRecord[];
  message: string;
}

export interface AgentMcpBinding {
  server_id: number;
  allowed_tools: string[] | null;
}

export const mcp = {
  listServers: () => invoke<McpServerRecord[]>("list_mcp_servers"),

  getServer: (id: number) => invoke<McpServerRecord | null>("get_mcp_server", { id }),

  upsertServer: (record: McpServerRecord) =>
    invoke<number>("upsert_mcp_server", { record }),

  deleteServer: (id: number) => invoke<boolean>("delete_mcp_server", { id }),

  testInput: (record: McpServerRecord) =>
    invoke<McpConnectionTestResult>("test_mcp_server_input", { record }),

  testConnection: (id: number) =>
    invoke<McpConnectionTestResult>("test_mcp_server_connection", { id }),

  listServerTools: (serverId: number) =>
    invoke<McpDiscoveredToolRecord[]>("list_mcp_server_tools", { serverId }),

  listAgentBindings: (agentId: number) =>
    invoke<AgentMcpBinding[]>("list_agent_mcp_bindings", { agentId }),

  setAgentBindings: (agentId: number, bindings: AgentMcpBinding[]) =>
    invoke<void>("set_agent_mcp_bindings", { agentId, bindings }),
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
    metadata?: {
      summary: boolean;
      mode?:
        | "compaction_prompt"
        | "compaction_summary"
        | "compaction_replay"
        | null;
      synthetic: boolean;
      collapsed_by_default: boolean;
    } | null;
  }>;
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

  updateThreadModel: (
    sessionId: string,
    threadId: string,
    providerPreferenceId: number,
    model: string,
  ) =>
    invoke<ChatSessionPayload>("update_thread_model", {
      sessionId,
      threadId,
      providerPreferenceId: providerPreferenceId.toString(),
      model,
    }),

  sendMessage: (sessionId: string, threadId: string, content: string) =>
    invoke<void>("send_message", { sessionId, threadId, content }),

  cancelTurn: (sessionId: string, threadId: string) =>
    invoke<void>("cancel_turn", { sessionId, threadId }),

  stopJob: (jobId: string) => invoke<void>("stop_job", { jobId }),

  getThreadSnapshot: (sessionId: string, threadId: string) =>
    invoke<ThreadSnapshotPayload>("get_thread_snapshot", {
      sessionId,
      threadId,
    }),

  listThreads: (sessionId: string) =>
    invoke<ThreadSummary[]>("list_threads", { sessionId }),
};

export const threadPool = {
  getSnapshot: () => invoke<ThreadPoolSnapshot>("get_thread_pool_snapshot"),
  getState: () => invoke<ThreadPoolState>("get_thread_pool_state"),
};
