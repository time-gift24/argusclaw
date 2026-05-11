export type ProviderSecretStatus = "ready" | "requires_reentry";
export type ProviderKind = "openai-compatible";
export type McpServerStatus =
  | "ready"
  | "connecting"
  | "retrying"
  | "failed"
  | "disabled";
export type RiskLevel = "low" | "medium" | "high" | "critical";

export interface ModelConfig {
  max_context_window: number;
}

export interface HealthResponse {
  status: string;
}

export interface CurrentUserResponse {
  id: string | null;
  external_id: string;
  display_name: string | null;
  is_admin: boolean;
}

export interface BootstrapResponse {
  instance_name: string;
  provider_count: number;
  template_count: number;
  mcp_server_count: number;
  default_provider_id: number | null;
  default_template_id: number | null;
  mcp_ready_count: number;
  current_user?: CurrentUserResponse;
}

export interface LlmProviderRecord {
  id: number;
  kind: ProviderKind;
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

export interface UpdateLlmProviderRequest extends Omit<LlmProviderRecord, "api_key"> {
  api_key: string | null;
}

export type SaveProviderRequest = LlmProviderRecord | UpdateLlmProviderRequest;

export interface AgentRecord {
  id: number;
  display_name: string;
  description: string;
  version: string;
  provider_id: number | null;
  model_id?: string | null;
  system_prompt: string;
  tool_names: string[];
  subagent_names: string[];
  max_tokens?: number | null;
  temperature?: number | null;
  thinking_config?: {
    type: "enabled" | "disabled";
    clear_thinking: boolean;
  } | null;
  mcp_bindings?: AgentMcpBinding[];
}

export interface AgentMcpBinding {
  server_id: number;
  allowed_tools?: string[] | null;
}

export interface ChatOptionsResponse {
  providers: LlmProviderRecord[];
  templates: AgentRecord[];
}

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
  request?: string | null;
  response?: string | null;
}

export interface McpDiscoveredToolRecord {
  server_id: number;
  tool_name_original: string;
  description: string;
  schema: unknown;
  annotations: unknown | null;
}

export interface McpConnectionTestResult {
  status: McpServerStatus;
  checked_at: string;
  latency_ms: number;
  discovered_tools: McpDiscoveredToolRecord[];
  message: string;
}

export interface ToolRegistryItem {
  name: string;
  description: string;
  risk_level: RiskLevel;
  parameters: unknown;
}

export interface DeleteResponse {
  deleted: boolean;
}

export interface AgentDeleteReport extends DeleteResponse {
  agent_deleted: boolean;
  deleted_job_count: number;
  deleted_run_count: number;
  deleted_thread_count: number;
  deleted_session_count: number;
}

export interface DeleteTemplateOptions {
  cascadeAssociations?: boolean;
}

export interface AccountStatus {
  configured: boolean;
  username: string | null;
}

export interface ConfigureAccountRequest {
  username: string;
  password: string;
}

export type ThreadRuntimeStatus =
  | "inactive"
  | "loading"
  | "queued"
  | "running"
  | "cooling"
  | "evicted";

export type ThreadPoolEventReason =
  | "cooling_expired"
  | "memory_pressure"
  | "cancelled"
  | "execution_failed";

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

export interface RuntimeStateResponse {
  thread_pool: ThreadPoolState;
  job_runtime: JobRuntimeState;
}

export type ChatMessageRole = "system" | "user" | "assistant" | "tool";

export interface ChatSessionSummary {
  id: string;
  name: string;
  thread_count: number;
  updated_at: string;
}

export interface ChatThreadSummary {
  id: string;
  title: string | null;
  turn_count: number;
  token_count: number;
  updated_at: string;
}

export interface ChatMessageRecord {
  role: ChatMessageRole;
  content: string;
  reasoning_content?: string | null;
  content_parts?: unknown[];
  tool_call_id?: string | null;
  name?: string | null;
  tool_calls?: unknown[] | null;
  metadata?: {
    summary?: boolean;
    mode?: "compaction_prompt" | "compaction_summary" | "compaction_replay" | null;
    synthetic?: boolean;
    collapsed_by_default?: boolean;
  } | null;
}

export interface CreateChatThreadRequest {
  template_id: number;
  provider_id: number | null;
  model: string | null;
}

export interface CreateChatSessionWithThreadRequest extends CreateChatThreadRequest {
  name?: string | null;
}

export interface UpdateChatThreadModelRequest {
  provider_id: number;
  model: string;
}

export interface ChatThreadSnapshot {
  session_id: string;
  thread_id: string;
  messages: ChatMessageRecord[];
  turn_count: number;
  token_count: number;
  plan_item_count: number;
}

export interface ChatThreadBinding {
  session_id: string;
  thread_id: string;
  template_id: number;
  effective_provider_id: number | null;
  effective_model: string | null;
}

export interface ChatSessionPayload extends ChatThreadBinding {
  session_key: string;
}

export interface ChatActionResponse {
  accepted: boolean;
}

export type ScheduledMessageStatus =
  | "pending"
  | "queued"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "paused";

export interface ScheduledMessageSummary {
  id: string;
  name: string;
  status: ScheduledMessageStatus;
  template_id: number;
  provider_id: number | null;
  model: string | null;
  last_session_id: string | null;
  last_thread_id: string | null;
  run_history: ScheduledMessageRunSummary[];
  prompt: string;
  cron_expr: string | null;
  scheduled_at: string | null;
  timezone: string | null;
  last_error: string | null;
}

export interface ScheduledMessageRunSummary {
  session_id: string;
  thread_id: string;
  created_at: string;
}

export interface CreateScheduledMessageRequest {
  template_id: number;
  provider_id?: number | null;
  model?: string | null;
  name: string;
  prompt: string;
  cron_expr?: string | null;
  scheduled_at?: string | null;
  timezone?: string | null;
}

export type UpdateScheduledMessageRequest = CreateScheduledMessageRequest;

export type AgentRunStatus = "queued" | "running" | "completed" | "failed";

export interface CreateAgentRunRequest {
  agent_id: number;
  prompt: string;
}

export interface AgentRunSummary {
  run_id: string;
  agent_id: number;
  status: AgentRunStatus;
  created_at: string;
  updated_at: string;
}

export interface AgentRunDetail extends AgentRunSummary {
  prompt: string;
  result: string | null;
  error: string | null;
  completed_at: string | null;
}

export type ChatThreadEventPayload =
  | {
      type: "reasoning_delta" | "content_delta";
      delta: string;
    }
  | {
      type: "retry_attempt";
      attempt: number;
      max_retries: number;
      error: string;
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
  | {
      type: "turn_failed";
      error: string;
    }
  | {
      type:
        | "turn_settled"
        | "idle"
        | "compaction_started"
        | "compaction_finished"
        | "compaction_failed"
        | "compacted"
        | "tool_call_delta"
        | "llm_usage"
        | "notice"
        | "thread_bound_to_job"
        | "thread_pool_queued"
        | "thread_pool_started"
        | "thread_pool_cooling"
        | "thread_pool_evicted"
        | "thread_pool_metrics_updated"
        | "job_runtime_queued"
        | "job_runtime_started"
        | "job_runtime_cooling"
        | "job_runtime_evicted"
        | "job_runtime_updated"
        | "job_runtime_metrics_updated"
        | "job_dispatched"
        | "job_result"
        | "mailbox_message_queued";
      [key: string]: unknown;
    };

export interface ChatThreadEventEnvelope {
  session_id: string;
  thread_id: string;
  turn_number: number | null;
  payload: ChatThreadEventPayload;
}

export interface ChatThreadEventHandlers {
  onOpen?(): void;
  onEvent(event: ChatThreadEventEnvelope): void;
  onError(error: Error): void;
}

export interface RuntimeEventHandlers {
  onSnapshot(snapshot: RuntimeStateResponse): void;
  onError(error: Error): void;
}

export interface RuntimeEventSubscription {
  close(): void;
}

interface MutationResponse<T> {
  item: T;
}

interface DataResponse<T> {
  data: T;
}

interface ErrorResponse {
  error?: {
    code?: string;
    message?: string;
  };
}

export interface ApiClient {
  getHealth(): Promise<HealthResponse>;
  getBootstrap(): Promise<BootstrapResponse>;
  getRuntimeState(): Promise<RuntimeStateResponse>;
  subscribeRuntimeState?(handlers: RuntimeEventHandlers): RuntimeEventSubscription;
  getAccountStatus?(): Promise<AccountStatus>;
  configureAccount?(input: ConfigureAccountRequest): Promise<AccountStatus>;
  listProviders(): Promise<LlmProviderRecord[]>;
  saveProvider(input: SaveProviderRequest): Promise<LlmProviderRecord>;
  deleteProvider?(providerId: number): Promise<DeleteResponse>;
  testProvider?(providerId: number, model?: string): Promise<ProviderTestResult>;
  testProviderDraft?(input: LlmProviderRecord): Promise<ProviderTestResult>;
  getChatOptions?(): Promise<ChatOptionsResponse>;
  listTemplates(): Promise<AgentRecord[]>;
  saveTemplate(input: AgentRecord): Promise<AgentRecord>;
  deleteTemplate?(
    templateId: number,
    options?: DeleteTemplateOptions,
  ): Promise<AgentDeleteReport>;
  listMcpServers(): Promise<McpServerRecord[]>;
  saveMcpServer(input: McpServerRecord): Promise<McpServerRecord>;
  deleteMcpServer?(serverId: number): Promise<DeleteResponse>;
  testMcpServer?(serverId: number): Promise<McpConnectionTestResult>;
  testMcpServerDraft?(input: McpServerRecord): Promise<McpConnectionTestResult>;
  listMcpServerTools?(serverId: number): Promise<McpDiscoveredToolRecord[]>;
  listTools?(): Promise<ToolRegistryItem[]>;
  listChatSessions?(): Promise<ChatSessionSummary[]>;
  createChatSessionWithThread?(input: CreateChatSessionWithThreadRequest): Promise<ChatSessionPayload>;
  createChatSession?(name: string): Promise<ChatSessionSummary>;
  renameChatSession?(sessionId: string, name: string): Promise<ChatSessionSummary>;
  deleteChatSession?(sessionId: string): Promise<DeleteResponse>;
  listChatThreads?(sessionId: string): Promise<ChatThreadSummary[]>;
  createChatThread?(sessionId: string, input: CreateChatThreadRequest): Promise<ChatThreadSummary>;
  renameChatThread?(sessionId: string, threadId: string, title: string): Promise<ChatThreadSummary>;
  deleteChatThread?(sessionId: string, threadId: string): Promise<DeleteResponse>;
  getChatThreadSnapshot?(sessionId: string, threadId: string): Promise<ChatThreadSnapshot>;
  updateChatThreadModel?(
    sessionId: string,
    threadId: string,
    input: UpdateChatThreadModelRequest,
  ): Promise<ChatThreadBinding>;
  activateChatThread?(sessionId: string, threadId: string): Promise<ChatThreadBinding>;
  listChatMessages?(sessionId: string, threadId: string): Promise<ChatMessageRecord[]>;
  sendChatMessage?(sessionId: string, threadId: string, message: string): Promise<ChatActionResponse>;
  cancelChatThread?(sessionId: string, threadId: string): Promise<ChatActionResponse>;
  subscribeChatThread?(sessionId: string, threadId: string, handlers: ChatThreadEventHandlers): RuntimeEventSubscription;
  listScheduledMessages?(): Promise<ScheduledMessageSummary[]>;
  createScheduledMessage?(input: CreateScheduledMessageRequest): Promise<ScheduledMessageSummary>;
  updateScheduledMessage?(id: string, input: UpdateScheduledMessageRequest): Promise<ScheduledMessageSummary>;
  pauseScheduledMessage?(id: string): Promise<ScheduledMessageSummary>;
  triggerScheduledMessage?(id: string): Promise<ScheduledMessageSummary>;
  deleteScheduledMessage?(id: string): Promise<DeleteResponse>;
  createAgentRun?(input: CreateAgentRunRequest): Promise<AgentRunSummary>;
  getAgentRun?(runId: string): Promise<AgentRunDetail>;
}

class HttpApiClient implements ApiClient {
  constructor(private readonly baseUrl = "/api/v1") {}

  getHealth(): Promise<HealthResponse> {
    return this.request("/health");
  }

  getBootstrap(): Promise<BootstrapResponse> {
    return this.request("/bootstrap");
  }

  getRuntimeState(): Promise<RuntimeStateResponse> {
    return this.request("/runtime");
  }

  getAccountStatus(): Promise<AccountStatus> {
    return this.request("/account");
  }

  configureAccount(input: ConfigureAccountRequest): Promise<AccountStatus> {
    return this.request("/account", {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method: "PUT",
    });
  }

  subscribeRuntimeState(handlers: RuntimeEventHandlers): RuntimeEventSubscription {
    const events = new EventSource(`${this.baseUrl}/runtime/events`, { withCredentials: true });

    events.addEventListener("runtime.snapshot", (event) => {
      try {
        handlers.onSnapshot(JSON.parse((event as MessageEvent<string>).data) as RuntimeStateResponse);
      } catch (reason) {
        handlers.onError(reason instanceof Error ? reason : new Error("运行状态事件解析失败。"));
      }
    });
    events.onerror = () => {
      handlers.onError(new Error("运行状态事件流连接失败，已切换为轮询。"));
    };

    return {
      close() {
        events.close();
      },
    };
  }

  listProviders(): Promise<LlmProviderRecord[]> {
    return this.request("/providers");
  }

  getChatOptions(): Promise<ChatOptionsResponse> {
    return this.request("/chat/options");
  }

  async saveProvider(input: SaveProviderRequest): Promise<LlmProviderRecord> {
    const path = input.id > 0 ? `/providers/${input.id}` : "/providers";
    const method = input.id > 0 ? "PATCH" : "POST";
    const response = await this.request<MutationResponse<LlmProviderRecord>>(path, {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method,
    });

    return response.item;
  }

  async deleteProvider(providerId: number): Promise<DeleteResponse> {
    const response = await this.request<MutationResponse<DeleteResponse>>(`/providers/${providerId}`, {
      method: "DELETE",
    });

    return response.item;
  }

  testProvider(providerId: number, model?: string): Promise<ProviderTestResult> {
    return this.request(`/providers/${providerId}/test`, {
      body: JSON.stringify({ model }),
      headers: {
        "Content-Type": "application/json",
      },
      method: "POST",
    });
  }

  testProviderDraft(input: LlmProviderRecord): Promise<ProviderTestResult> {
    return this.request("/providers/test", {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method: "POST",
    });
  }

  listTemplates(): Promise<AgentRecord[]> {
    return this.request("/agents/templates");
  }

  async saveTemplate(input: AgentRecord): Promise<AgentRecord> {
    const path = input.id > 0 ? `/agents/templates/${input.id}` : "/agents/templates";
    const method = input.id > 0 ? "PATCH" : "POST";
    const response = await this.request<MutationResponse<AgentRecord>>(path, {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method,
    });

    return response.item;
  }

  async deleteTemplate(
    templateId: number,
    options: DeleteTemplateOptions = {},
  ): Promise<AgentDeleteReport> {
    const query = options.cascadeAssociations ? "?cascade_associations=true" : "";
    const response = await this.request<MutationResponse<AgentDeleteReport>>(
      `/agents/templates/${templateId}${query}`,
      {
        method: "DELETE",
      },
    );

    return response.item;
  }

  listMcpServers(): Promise<McpServerRecord[]> {
    return this.request("/mcp/servers");
  }

  async saveMcpServer(input: McpServerRecord): Promise<McpServerRecord> {
    const path = input.id ? `/mcp/servers/${input.id}` : "/mcp/servers";
    const method = input.id ? "PATCH" : "POST";
    const response = await this.request<MutationResponse<McpServerRecord>>(path, {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method,
    });

    return response.item;
  }

  async deleteMcpServer(serverId: number): Promise<DeleteResponse> {
    const response = await this.request<MutationResponse<DeleteResponse>>(`/mcp/servers/${serverId}`, {
      method: "DELETE",
    });

    return response.item;
  }

  testMcpServer(serverId: number): Promise<McpConnectionTestResult> {
    return this.request(`/mcp/servers/${serverId}/test`, {
      method: "POST",
    });
  }

  testMcpServerDraft(input: McpServerRecord): Promise<McpConnectionTestResult> {
    return this.request("/mcp/servers/test", {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method: "POST",
    });
  }

  listMcpServerTools(serverId: number): Promise<McpDiscoveredToolRecord[]> {
    return this.request(`/mcp/servers/${serverId}/tools`);
  }

  listTools(): Promise<ToolRegistryItem[]> {
    return this.request("/tools");
  }

  async createAgentRun(input: CreateAgentRunRequest): Promise<AgentRunSummary> {
    const response = await this.request<DataResponse<AgentRunSummary>>("/agents/runs", {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method: "POST",
    });

    return response.data;
  }

  getAgentRun(runId: string): Promise<AgentRunDetail> {
    return this.request(`/agents/runs/${runId}`);
  }

  listChatSessions(): Promise<ChatSessionSummary[]> {
    return this.request("/chat/sessions");
  }

  async createChatSessionWithThread(input: CreateChatSessionWithThreadRequest): Promise<ChatSessionPayload> {
    const response = await this.request<MutationResponse<ChatSessionPayload>>("/chat/sessions/with-thread", {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method: "POST",
    });

    return response.item;
  }

  async createChatSession(name: string): Promise<ChatSessionSummary> {
    const response = await this.request<MutationResponse<ChatSessionSummary>>("/chat/sessions", {
      body: JSON.stringify({ name }),
      headers: {
        "Content-Type": "application/json",
      },
      method: "POST",
    });

    return response.item;
  }

  async renameChatSession(sessionId: string, name: string): Promise<ChatSessionSummary> {
    const response = await this.request<MutationResponse<ChatSessionSummary>>(`/chat/sessions/${sessionId}`, {
      body: JSON.stringify({ name }),
      headers: {
        "Content-Type": "application/json",
      },
      method: "PATCH",
    });

    return response.item;
  }

  async deleteChatSession(sessionId: string): Promise<DeleteResponse> {
    const response = await this.request<MutationResponse<DeleteResponse>>(`/chat/sessions/${sessionId}`, {
      method: "DELETE",
    });

    return response.item;
  }

  listChatThreads(sessionId: string): Promise<ChatThreadSummary[]> {
    return this.request(`/chat/sessions/${sessionId}/threads`);
  }

  async createChatThread(sessionId: string, input: CreateChatThreadRequest): Promise<ChatThreadSummary> {
    const response = await this.request<MutationResponse<ChatThreadSummary>>(`/chat/sessions/${sessionId}/threads`, {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method: "POST",
    });

    return response.item;
  }

  async renameChatThread(sessionId: string, threadId: string, title: string): Promise<ChatThreadSummary> {
    const response = await this.request<MutationResponse<ChatThreadSummary>>(
      `/chat/sessions/${sessionId}/threads/${threadId}`,
      {
        body: JSON.stringify({ title }),
        headers: {
          "Content-Type": "application/json",
        },
        method: "PATCH",
      },
    );

    return response.item;
  }

  async deleteChatThread(sessionId: string, threadId: string): Promise<DeleteResponse> {
    const response = await this.request<MutationResponse<DeleteResponse>>(
      `/chat/sessions/${sessionId}/threads/${threadId}`,
      {
        method: "DELETE",
      },
    );

    return response.item;
  }

  getChatThreadSnapshot(sessionId: string, threadId: string): Promise<ChatThreadSnapshot> {
    return this.request(`/chat/sessions/${sessionId}/threads/${threadId}`);
  }

  async updateChatThreadModel(
    sessionId: string,
    threadId: string,
    input: UpdateChatThreadModelRequest,
  ): Promise<ChatThreadBinding> {
    const response = await this.request<MutationResponse<ChatThreadBinding>>(
      `/chat/sessions/${sessionId}/threads/${threadId}/model`,
      {
        body: JSON.stringify(input),
        headers: {
          "Content-Type": "application/json",
        },
        method: "PATCH",
      },
    );

    return response.item;
  }

  async activateChatThread(sessionId: string, threadId: string): Promise<ChatThreadBinding> {
    const response = await this.request<MutationResponse<ChatThreadBinding>>(
      `/chat/sessions/${sessionId}/threads/${threadId}/activate`,
      {
        method: "POST",
      },
    );

    return response.item;
  }

  listChatMessages(sessionId: string, threadId: string): Promise<ChatMessageRecord[]> {
    return this.request(`/chat/sessions/${sessionId}/threads/${threadId}/messages`);
  }

  async sendChatMessage(sessionId: string, threadId: string, message: string): Promise<ChatActionResponse> {
    const response = await this.request<MutationResponse<ChatActionResponse>>(
      `/chat/sessions/${sessionId}/threads/${threadId}/messages`,
      {
        body: JSON.stringify({ message }),
        headers: {
          "Content-Type": "application/json",
        },
        method: "POST",
      },
    );

    return response.item;
  }

  async cancelChatThread(sessionId: string, threadId: string): Promise<ChatActionResponse> {
    const response = await this.request<MutationResponse<ChatActionResponse>>(
      `/chat/sessions/${sessionId}/threads/${threadId}/cancel`,
      {
        method: "POST",
      },
    );

    return response.item;
  }

  listScheduledMessages(): Promise<ScheduledMessageSummary[]> {
    return this.request("/scheduled-messages");
  }

  async createScheduledMessage(input: CreateScheduledMessageRequest): Promise<ScheduledMessageSummary> {
    const response = await this.request<MutationResponse<ScheduledMessageSummary>>("/scheduled-messages", {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method: "POST",
    });

    return response.item;
  }

  async updateScheduledMessage(id: string, input: UpdateScheduledMessageRequest): Promise<ScheduledMessageSummary> {
    const response = await this.request<MutationResponse<ScheduledMessageSummary>>(`/scheduled-messages/${id}`, {
      body: JSON.stringify(input),
      headers: {
        "Content-Type": "application/json",
      },
      method: "PUT",
    });

    return response.item;
  }

  async pauseScheduledMessage(id: string): Promise<ScheduledMessageSummary> {
    const response = await this.request<MutationResponse<ScheduledMessageSummary>>(`/scheduled-messages/${id}/pause`, {
      method: "POST",
    });

    return response.item;
  }

  async triggerScheduledMessage(id: string): Promise<ScheduledMessageSummary> {
    const response = await this.request<MutationResponse<ScheduledMessageSummary>>(`/scheduled-messages/${id}/trigger`, {
      method: "POST",
    });

    return response.item;
  }

  async deleteScheduledMessage(id: string): Promise<DeleteResponse> {
    const response = await this.request<MutationResponse<DeleteResponse>>(`/scheduled-messages/${id}`, {
      method: "DELETE",
    });

    return response.item;
  }

  subscribeChatThread(
    sessionId: string,
    threadId: string,
    handlers: ChatThreadEventHandlers,
  ): RuntimeEventSubscription {
    const events = new EventSource(`${this.baseUrl}/chat/sessions/${sessionId}/threads/${threadId}/events`, { withCredentials: true });

    events.onopen = () => {
      handlers.onOpen?.();
    };
    events.addEventListener("chat.thread_event", (event) => {
      try {
        handlers.onEvent(JSON.parse((event as MessageEvent<string>).data) as ChatThreadEventEnvelope);
      } catch (reason) {
        handlers.onError(reason instanceof Error ? reason : new Error("对话事件解析失败。"));
      }
    });
    events.addEventListener("chat.error", (event) => {
      handlers.onError(new Error((event as MessageEvent<string>).data || "对话事件流返回错误。"));
    });
    events.onerror = () => {
      handlers.onError(new Error("对话事件流连接失败，已切换为刷新兜底。"));
    };

    return {
      close() {
        events.close();
      },
    };
  }

  private async request<T>(path: string, init?: RequestInit): Promise<T> {
    const requestInit: RequestInit = {
      ...init,
      credentials: init?.credentials ?? "same-origin",
    };
    const response = await fetch(`${this.baseUrl}${path}`, requestInit);
    if (!response.ok) {
      if (response.status === 401) {
        this.redirectToLogin();
      }
      throw new Error(await this.readErrorMessage(response));
    }

    return (await response.json()) as T;
  }

  private redirectToLogin(): void {
    if (typeof window === "undefined") {
      return;
    }
    const current = `${window.location.pathname}${window.location.search}${window.location.hash}`;
    window.location.assign(`/auth/login?next=${encodeURIComponent(current)}`);
  }

  private async readErrorMessage(response: Response): Promise<string> {
    const fallback = `Request failed: ${response.status}`;
    const contentType = response.headers.get("content-type") ?? "";
    if (!contentType.includes("application/json")) {
      return fallback;
    }

    try {
      const body = (await response.json()) as ErrorResponse;
      const message = body.error?.message?.trim();
      return message || fallback;
    } catch {
      return fallback;
    }
  }
}

let apiClient: ApiClient = new HttpApiClient();

export function getApiClient(): ApiClient {
  return apiClient;
}

export function setApiClient(client: ApiClient): void {
  apiClient = client;
}

export function resetApiClient(): void {
  apiClient = new HttpApiClient();
}
