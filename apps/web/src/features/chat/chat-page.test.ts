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
  type ChatThreadSnapshot,
  type ChatThreadSummary,
  type LlmProviderRecord,
  type RuntimeEventSubscription,
  type ChatThreadEventHandlers,
} from "@/lib/api";

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
    listProviders: vi.fn().mockResolvedValue([provider()]),
    listTemplates: vi.fn().mockResolvedValue([template()]),
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
    subscribeChatThread: vi.fn().mockReturnValue({ close: vi.fn() }),
    ...overrides,
  } as ApiClient;
}

afterEach(() => {
  resetApiClient();
});

describe("ChatPage", () => {
  it("does not render the legacy left sidebar", async () => {
    setApiClient(makeApiClient({ listChatSessions: vi.fn().mockResolvedValue([]) }));
    const wrapper = mount(ChatPage);
    await flushPromises();

    // No chat-sidebar element
    expect(wrapper.find(".chat-sidebar").exists()).toBe(false);
  });

  it("shows template selector in the composer bar header", async () => {
    const listTemplates = vi.fn().mockResolvedValue([
      template(),
      template({ id: 9, display_name: "代码助手" }),
    ]);
    setApiClient(
      makeApiClient({
        listTemplates,
        listChatSessions: vi.fn().mockResolvedValue([]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    // Template selector should be in the composer bar
    expect(wrapper.find(".composer-bar").exists()).toBe(true);
  });

  it("opens history dialog when clicking the history button", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session(), session({ id: "session-2", name: "另一个会话" })]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    // Dialog should not be open initially
    expect(document.querySelector(".history-dialog")).toBeNull();

    // Click history button
    const historyBtn = wrapper.findAll("button").find((b) => b.text().includes("历史"));
    expect(historyBtn).toBeDefined();
    await historyBtn!.trigger("click");
    await flushPromises();

    // Dialog should be open (teleported to body)
    const dialog = document.querySelector(".history-dialog");
    expect(dialog).not.toBeNull();
    expect(dialog?.textContent).toContain("会话列表");
    expect(dialog?.textContent).toContain("默认会话");
    expect(dialog?.textContent).toContain("另一个会话");
  });

  it("shows appropriate placeholder when no active thread vs when thread is active", async () => {
    // No thread - shows draft placeholder
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([]),
        listTemplates: vi.fn().mockResolvedValue([template()]),
        listProviders: vi.fn().mockResolvedValue([provider()]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    const senderNoThread = wrapper.findComponent({ name: "TrSender" });
    expect((senderNoThread.vm.$props as Record<string, unknown>).placeholder).toContain("输入第一条消息");

    // With active thread - shows normal placeholder
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([]),
      }),
    );
    const wrapper2 = mount(ChatPage);
    await flushPromises();

    const senderWithThread = wrapper2.findComponent({ name: "TrSender" });
    expect((senderWithThread.vm.$props as Record<string, unknown>).placeholder).toBe("输入消息，Enter 发送");
  });

  it("shows conversation title in header when thread is active", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread({ title: "产品讨论" })]),
        listChatMessages: vi.fn().mockResolvedValue([]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.text()).toContain("产品讨论");
  });

  it("starts a new chat draft when clicking the new chat button", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([message("user", "旧消息"), message("assistant", "旧回复")]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    // Should show the thread title
    expect(wrapper.text()).toContain("产品讨论");

    // Click new chat button
    const newChatBtn = wrapper.findAll("button").find((b) => b.text().includes("新对话"));
    expect(newChatBtn).toBeDefined();
    await newChatBtn!.trigger("click");
    await flushPromises();

    // Should show prompt panel (no active thread)
    expect(wrapper.find(".prompt-panel").exists()).toBe(true);
  });
});
