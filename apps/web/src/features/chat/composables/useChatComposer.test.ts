import { describe, expect, it, vi } from "vitest";
import { ref } from "vue";

import {
  resetApiClient,
  setApiClient,
  type ApiClient,
  type AgentRecord,
  type ChatMessageRecord,
  type ChatSessionPayload,
  type ChatThreadBinding,
  type ChatThreadSummary,
  type LlmProviderRecord,
} from "@/lib/api";
import { useChatComposer } from "./useChatComposer";

function provider(overrides: Partial<LlmProviderRecord> = {}): LlmProviderRecord {
  return {
    id: 7,
    kind: "openai-compatible",
    display_name: "Z.ai",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
    api_key: "",
    models: ["glm-4.7"],
    model_config: {},
    default_model: "glm-4.7",
    is_default: true,
    extra_headers: {},
    secret_status: "ready",
    meta_data: {},
    ...overrides,
  };
}

function template(overrides: Partial<AgentRecord> = {}): AgentRecord {
  return {
    id: 3,
    display_name: "通用助手",
    description: "适合日常问答和轻量任务。",
    version: "1.0.0",
    provider_id: null,
    model_id: null,
    system_prompt: "You are helpful.",
    tool_names: [],
    subagent_names: [],
    max_tokens: null,
    temperature: null,
    thinking_config: null,
    ...overrides,
  };
}

function createComposer(apiOverrides: Partial<ApiClient> = {}) {
  const activeSessionId = ref("session-1");
  const activeThreadId = ref("thread-1");
  const activeBinding = ref<ChatThreadBinding | null>(null);
  const selectedTemplateId = ref<number | null>(3);
  const selectedProviderId = ref<number | null>(7);
  const selectedModel = ref("glm-4.7");
  const providers = ref<LlmProviderRecord[]>([provider()]);
  const templates = ref<AgentRecord[]>([template()]);
  const sessionName = ref("默认会话");
  const threadTitle = ref("线程 1");
  const threads = ref<ChatThreadSummary[]>([]);
  const streaming = ref(false);
  const assistantCountAtStreamStart = ref(0);
  const messages = ref<ChatMessageRecord[]>([]);

  const refreshSessions = vi.fn().mockResolvedValue(undefined);
  const refreshThreads = vi.fn().mockResolvedValue(undefined);
  const applyChatSessionPayload = vi.fn<(payload: ChatSessionPayload) => void>();
  const openThreadEvents = vi.fn();
  const closeThreadEvents = vi.fn();
  const resetRuntimeActivity = vi.fn();
  const refreshStreamUntilSettled = vi.fn().mockResolvedValue(undefined);
  const countAssistantMessages = vi.fn(() => messages.value.filter((message) => message.role === "assistant").length);
  const clearPendingAssistant = vi.fn();

  setApiClient({
    getHealth: vi.fn().mockResolvedValue({ status: "ok" }),
    getBootstrap: vi.fn().mockResolvedValue({
      instance_name: "ArgusWing",
      provider_count: 1,
      template_count: 1,
      mcp_server_count: 0,
      default_provider_id: 7,
      default_template_id: 3,
      mcp_ready_count: 0,
    }),
    listProviders: vi.fn().mockResolvedValue([provider()]),
    saveProvider: vi.fn(),
    listTemplates: vi.fn().mockResolvedValue([template()]),
    listMcpServers: vi.fn().mockResolvedValue([]),
    saveMcpServer: vi.fn(),
    saveTemplate: vi.fn(),
    listTools: vi.fn().mockResolvedValue([]),
    sendChatMessage: vi.fn().mockResolvedValue({ accepted: true }),
    ...apiOverrides,
  } as ApiClient);

  const composer = useChatComposer({
    activeSessionId,
    activeThreadId,
    activeBinding,
    selectedTemplateId,
    selectedProviderId,
    selectedModel,
    providers,
    templates,
    sessionName,
    threadTitle,
    threads,
    refreshSessions,
    refreshThreads,
    applyChatSessionPayload,
    openThreadEvents,
    closeThreadEvents,
    resetRuntimeActivity,
    refreshStreamUntilSettled,
    countAssistantMessages,
    clearPendingAssistant,
    streaming,
    assistantCountAtStreamStart,
    messages,
  });

  return {
    composer,
    openThreadEvents,
    closeThreadEvents,
    messages,
    streaming,
    refreshStreamUntilSettled,
    clearPendingAssistant,
  };
}

describe("useChatComposer", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("removes the optimistic user message when send fails", async () => {
    const { composer, messages, streaming, closeThreadEvents, refreshStreamUntilSettled } = createComposer({
      sendChatMessage: vi.fn().mockRejectedValue(new Error("send failed")),
      subscribeChatThread: vi.fn().mockReturnValue({ close: vi.fn() }),
    });

    await composer.sendMessage("继续");

    expect(messages.value).toEqual([]);
    expect(streaming.value).toBe(false);
    expect(closeThreadEvents).toHaveBeenCalledTimes(2);
    expect(refreshStreamUntilSettled).not.toHaveBeenCalled();
    expect(composer.error.value).toBe("send failed");
  });
});
