import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));
vi.mock("@opentiny/tiny-robot", async () => import("@/test/stubs/tiny-robot"));

import ChatPage from "./ChatPage.vue";
import {
  resetApiClient,
  setApiClient,
  type AgentRecord,
  type ApiClient,
  type ChatActionResponse,
  type ChatMessageRecord,
  type ChatSessionPayload,
  type ChatSessionSummary,
  type ChatThreadBinding,
  type ChatThreadEventHandlers,
  type ChatThreadSnapshot,
  type ChatThreadSummary,
  type LlmProviderRecord,
  type RuntimeEventSubscription,
} from "@/lib/api";

const capturedConsoleErrors: unknown[] = [];

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

function session(overrides: Partial<ChatSessionSummary> = {}): ChatSessionSummary {
  return {
    id: "session-1",
    name: "默认会话",
    thread_count: 1,
    updated_at: "2026-04-24T10:00:00Z",
    ...overrides,
  };
}

function thread(overrides: Partial<ChatThreadSummary> = {}): ChatThreadSummary {
  return {
    id: "thread-1",
    title: "产品讨论",
    turn_count: 1,
    token_count: 12,
    updated_at: "2026-04-24T10:01:00Z",
    ...overrides,
  };
}

function message(
  role: ChatMessageRecord["role"],
  content: string,
  overrides: Partial<ChatMessageRecord> = {},
): ChatMessageRecord {
  return {
    role,
    content,
    reasoning_content: null,
    content_parts: [],
    tool_call_id: null,
    name: null,
    tool_calls: null,
    metadata: null,
    ...overrides,
  };
}

function runtimeState() {
  return {
    thread_pool: {
      snapshot: {
        max_threads: 8,
        active_threads: 0,
        queued_threads: 0,
        running_threads: 0,
        cooling_threads: 0,
        evicted_threads: 0,
        estimated_memory_bytes: 0,
        peak_estimated_memory_bytes: 0,
        process_memory_bytes: null,
        peak_process_memory_bytes: null,
        resident_thread_count: 0,
        avg_thread_memory_bytes: 0,
        captured_at: "2026-04-24T10:00:00Z",
      },
      runtimes: [],
    },
    job_runtime: {
      snapshot: {
        max_threads: 8,
        active_threads: 0,
        queued_threads: 0,
        running_threads: 0,
        cooling_threads: 0,
        evicted_threads: 0,
        estimated_memory_bytes: 0,
        peak_estimated_memory_bytes: 0,
        process_memory_bytes: null,
        peak_process_memory_bytes: null,
        resident_thread_count: 0,
        avg_thread_memory_bytes: 0,
        captured_at: "2026-04-24T10:00:00Z",
      },
      runtimes: [],
    },
  };
}

function makeApiClient(overrides: Partial<ApiClient> = {}): ApiClient {
  return {
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
    getRuntimeState: vi.fn().mockResolvedValue(runtimeState()),
    getSettings: vi.fn().mockResolvedValue({
      instance_name: "ArgusWing",
      default_provider_id: 7,
      default_provider_name: "Z.ai",
    }),
    updateSettings: vi.fn().mockResolvedValue({
      instance_name: "ArgusWing",
      default_provider_id: 7,
      default_provider_name: "Z.ai",
    }),
    listProviders: vi.fn().mockResolvedValue([provider()]),
    saveProvider: vi.fn().mockImplementation(async (input) => input),
    listTemplates: vi.fn().mockResolvedValue([template()]),
    saveTemplate: vi.fn().mockImplementation(async (input) => input),
    listMcpServers: vi.fn().mockResolvedValue([]),
    saveMcpServer: vi.fn().mockImplementation(async (input) => input),
    listChatSessions: vi.fn().mockResolvedValue([]),
    createChatSessionWithThread: vi.fn().mockResolvedValue({
      session_key: "session-1",
      session_id: "session-1",
      template_id: 3,
      thread_id: "thread-1",
      effective_provider_id: 7,
      effective_model: "glm-4.7",
    }),
    createChatSession: vi.fn().mockResolvedValue(session()),
    renameChatSession: vi.fn().mockImplementation(async (_sessionId, name) => session({ name })),
    deleteChatSession: vi.fn().mockResolvedValue({ deleted: true }),
    listChatThreads: vi.fn().mockResolvedValue([]),
    createChatThread: vi.fn().mockResolvedValue(thread()),
    renameChatThread: vi.fn().mockImplementation(async (_sessionId, _threadId, title) => thread({ title })),
    deleteChatThread: vi.fn().mockResolvedValue({ deleted: true }),
    getChatThreadSnapshot: vi.fn().mockResolvedValue({
      session_id: "session-1",
      thread_id: "thread-1",
      messages: [],
      turn_count: 0,
      token_count: 0,
      plan_item_count: 0,
    }),
    updateChatThreadModel: vi.fn().mockResolvedValue({
      session_id: "session-1",
      thread_id: "thread-1",
      template_id: 3,
      effective_provider_id: 7,
      effective_model: "glm-4.7",
    }),
    activateChatThread: vi.fn().mockResolvedValue({
      session_id: "session-1",
      thread_id: "thread-1",
      template_id: 3,
      effective_provider_id: 7,
      effective_model: "glm-4.7",
    }),
    listChatMessages: vi.fn().mockResolvedValue([]),
    sendChatMessage: vi.fn().mockResolvedValue({ accepted: true }),
    cancelChatThread: vi.fn().mockResolvedValue({ accepted: true }),
    ...overrides,
  };
}

afterEach(() => {
  resetApiClient();
  capturedConsoleErrors.splice(0);
});

describe("ChatPage", () => {
  it("materializes a desktop-style session/thread on first send and renders streamed content in the active bubble", async () => {
    const listTemplates = vi.fn().mockResolvedValue([
      template(),
      template({
        id: 9,
        display_name: "代码助手",
        description: "适合代码审查和实现任务。",
        version: "1.1.0",
      }),
    ]);
    const listChatSessions = vi.fn<() => Promise<ChatSessionSummary[]>>().mockResolvedValueOnce([]).mockResolvedValue([session()]);
    const listChatThreads = vi.fn<(sessionId: string) => Promise<ChatThreadSummary[]>>().mockResolvedValue([thread()]);
    const createChatSessionWithThread = vi
      .fn<
        (input: {
          name: string;
          template_id: number;
          provider_id: number | null;
          model: string | null;
        }) => Promise<ChatSessionPayload>
      >()
      .mockResolvedValue({
        session_key: "9::7",
        session_id: "session-1",
        template_id: 9,
        thread_id: "thread-1",
        effective_provider_id: 7,
        effective_model: "glm-4.7",
      });
    const listChatMessages = vi
      .fn<(sessionId: string, threadId: string) => Promise<ChatMessageRecord[]>>()
      .mockResolvedValueOnce([])
      .mockResolvedValue([message("user", "帮我总结 MCP 配置"), message("assistant", "可以，先检查已启用服务。")]);
    const getChatThreadSnapshot = vi
      .fn<(sessionId: string, threadId: string) => Promise<ChatThreadSnapshot>>()
      .mockResolvedValue({
        session_id: "session-1",
        thread_id: "thread-1",
        messages: [message("user", "帮我总结 MCP 配置"), message("assistant", "可以，先检查已启用服务。")],
        turn_count: 1,
        token_count: 18,
        plan_item_count: 0,
      });
    const sendChatMessage = vi
      .fn<(sessionId: string, threadId: string, messageText: string) => Promise<ChatActionResponse>>()
      .mockResolvedValue({ accepted: true });
    const streamHandlers: ChatThreadEventHandlers[] = [];
    const subscribeChatThread = vi.fn(
      (_sessionId: string, _threadId: string, handlers: ChatThreadEventHandlers): RuntimeEventSubscription => {
        streamHandlers.push(handlers);
        handlers.onEvent({
          session_id: "session-1",
          thread_id: "thread-1",
          turn_number: 1,
          payload: {
            type: "content_delta",
            delta: "流式片段",
          },
        });

        return { close: vi.fn() };
      },
    );

    setApiClient(
      makeApiClient({
        listTemplates,
        listChatSessions,
        listChatThreads,
        createChatSessionWithThread,
        listChatMessages,
        getChatThreadSnapshot,
        sendChatMessage,
        subscribeChatThread,
      }),
    );

    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.text()).toContain("暂无对话会话");
    expect(wrapper.text()).toContain("适合日常问答和轻量任务。");
    await wrapper.get('[data-testid="conversation-template-select"]').setValue("9");
    await flushPromises();
    expect(wrapper.text()).toContain("适合代码审查和实现任务。");

    await wrapper.get("[data-testid='chat-input']").setValue("帮我总结 MCP 配置");
    await flushPromises();
    await wrapper.get(".tr-sender-stub button").trigger("click");
    await flushPromises();

    expect(createChatSessionWithThread).toHaveBeenCalledWith({
      name: "新的 Web 对话",
      template_id: 9,
      provider_id: 7,
      model: "glm-4.7",
    });
    expect(subscribeChatThread).toHaveBeenCalledWith("session-1", "thread-1", expect.any(Object));
    expect(sendChatMessage).toHaveBeenCalledWith("session-1", "thread-1", "帮我总结 MCP 配置");
    expect(wrapper.text()).toContain("消息已提交");
    expect(wrapper.text()).toContain("流式片段");

    const handlers = streamHandlers[0];
    if (!handlers) {
      throw new Error("chat event handlers should be registered");
    }
    handlers.onEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: 1,
      payload: {
        type: "turn_settled",
      },
    });
    await flushPromises();

    expect(wrapper.text()).toContain("可以，先检查已启用服务。");
  });

  it("cancels an active thread and surfaces API failures", async () => {
    const cancelChatThread = vi.fn<(sessionId: string, threadId: string) => Promise<ChatActionResponse>>().mockResolvedValue({
      accepted: true,
    });
    const failingListChatMessages = vi
      .fn<(sessionId: string, threadId: string) => Promise<ChatMessageRecord[]>>()
      .mockRejectedValue(new Error("Request failed: 502"));

    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: failingListChatMessages,
        cancelChatThread,
      }),
    );

    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.text()).toContain("Request failed: 502");
    await wrapper.get('[data-testid="cancel-thread"]').trigger("click");
    await flushPromises();

    expect(cancelChatThread).toHaveBeenCalledWith("session-1", "thread-1");
    expect(wrapper.text()).toContain("已请求取消当前线程。");
  });

  it("keeps template and provider selectors usable when chat session loading fails", async () => {
    const listProviders = vi.fn().mockResolvedValue([provider()]);
    const listTemplates = vi.fn().mockResolvedValue([template()]);
    const listChatSessions = vi.fn().mockRejectedValue(new Error("Request failed: 404"));

    setApiClient(
      makeApiClient({
        listProviders,
        listTemplates,
        listChatSessions,
      }),
    );

    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(listProviders).toHaveBeenCalledOnce();
    expect(listTemplates).toHaveBeenCalledOnce();
    expect(listChatSessions).toHaveBeenCalledOnce();
    expect(wrapper.text()).toContain("对话会话加载失败：Request failed: 404");
    expect(wrapper.text()).toContain("适合日常问答和轻量任务。");
    expect((wrapper.get('[data-testid="conversation-template-select"]').element as HTMLSelectElement).value).toBe("3");
    expect((wrapper.get('select[name="provider"]').element as HTMLSelectElement).value).toBe("7");
  });

  it("renders legacy blank sessions and assistant tool calls with useful labels", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session({ id: "session-empty-name", name: "" })]),
        listChatThreads: vi.fn().mockResolvedValue([thread({ id: "thread-empty-title", title: null })]),
        listChatMessages: vi.fn().mockResolvedValue([
          message("assistant", "", {
            tool_calls: [
              {
                id: "call-1",
                name: "scheduler",
                arguments: { action: "list_subagents" },
              },
            ],
          }),
        ]),
      }),
    );

    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.text()).toContain("会话 session-");
    expect(wrapper.text()).toContain("线程 thread-e");
    expect(wrapper.text()).toContain("正在调用工具：scheduler");
    expect(wrapper.text()).not.toContain("消息内容为空。");

    await wrapper.get('[data-testid="create-session"]').trigger("click");
    expect((wrapper.get('input[name="session-name"]').element as HTMLInputElement).value).toBe("新的 Web 对话");
  });

  it("shows starter prompts and applies a prompt to the sender", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([]),
      }),
    );

    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.text()).toContain("快速开始");
    await wrapper.get('[data-testid="prompt-provider"]').trigger("click");
    expect((wrapper.get("[data-testid='chat-input']").element as HTMLInputElement).value).toContain("当前默认模型");
  });
});
