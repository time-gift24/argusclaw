import { flushPromises, mount } from "@vue/test-utils";
import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));
vi.mock("@opentiny/tiny-robot", async () => import("@/test/stubs/tiny-robot"));
const routerPush = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const routeState = vi.hoisted(() => ({
  query: {} as Record<string, unknown>,
}));
vi.mock("vue-router", () => ({
  useRoute: () => routeState,
  useRouter: () => ({ push: routerPush }),
}));

import ChatPage from "./ChatPage.vue";
import ChatConversationPanel from "./components/ChatConversationPanel.vue";
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
  type ChatThreadJobSummary,
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

function dispatchedJob(overrides: Partial<ChatThreadJobSummary> = {}): ChatThreadJobSummary {
  return {
    job_id: "job-1",
    title: "整理会议纪要",
    subagent_name: "researcher",
    status: "succeeded",
    created_at: "2026-05-11T09:00:00Z",
    updated_at: "2026-05-11T09:02:00Z",
    result_preview: "已完成纪要整理",
    bound_thread_id: "thread-job-1",
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
    listChatThreadJobs: vi.fn().mockResolvedValue([]),
    ...overrides,
  } as ApiClient;
}

function createDeferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

async function chooseAgent(wrapper: ReturnType<typeof mount>, templateId: number) {
  await wrapper.get("[data-testid='agent-picker-trigger']").trigger("click");
  await wrapper.get(`[data-testid='agent-option-${templateId}']`).trigger("click");
  await flushPromises();
}

afterEach(() => {
  routerPush.mockClear();
  routeState.query = {};
  resetApiClient();
  document.body.innerHTML = "";
});

describe("ChatPage", () => {
  it("keeps a pending assistant bubble visible after sending the first message on a new chat", async () => {
    let resolveSend!: (value: ChatActionResponse) => void;
    const sendChatMessage = vi.fn().mockImplementation(
      () =>
        new Promise<ChatActionResponse>((resolve) => {
          resolveSend = resolve;
        }),
    );

    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([]),
        listChatThreads: vi.fn().mockResolvedValue([]),
        listChatMessages: vi.fn().mockResolvedValue([]),
        sendChatMessage,
        subscribeChatThread: vi.fn().mockReturnValue({ close: vi.fn() }),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    await wrapper.get("[data-testid='chat-input']").setValue("首条消息");
    await wrapper.get("[data-testid='chat-input']").trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(sendChatMessage).toHaveBeenCalledTimes(1);
    expect(wrapper.findAll(".tr-bubble-stub[data-role='assistant']")).toHaveLength(1);

    resolveSend({ accepted: true });
    await flushPromises();
  });

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

  it("renders selected agent and LLM values in composer chooser buttons", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([]),
        listTemplates: vi.fn().mockResolvedValue([template()]),
        listProviders: vi.fn().mockResolvedValue([provider()]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.findAllComponents({ name: "OpenTinySelectStub" })).toHaveLength(0);
    expect(wrapper.get("[data-testid='agent-picker-trigger']").text()).toContain("通用助手");
    expect(wrapper.get("[data-testid='llm-picker-trigger']").text()).toContain("Z.ai");
    expect(wrapper.get("[data-testid='llm-picker-trigger']").text()).toContain("glm-4.7");
  });

  it("renders the provider and model configured by the selected template on initial load", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([]),
        listTemplates: vi.fn().mockResolvedValue([
          template({ id: 9, display_name: "代码助手", provider_id: 8, model_id: "kimi-k2" }),
        ]),
        listProviders: vi.fn().mockResolvedValue([
          provider({ id: 7, display_name: "默认提供方", default_model: "glm-4.7", models: ["glm-4.7"] }),
          provider({ id: 8, display_name: "模板提供方", default_model: "kimi-default", models: ["kimi-default", "kimi-k2"] }),
        ]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.get("[data-testid='agent-picker-trigger']").text()).toContain("代码助手");
    expect(wrapper.get("[data-testid='llm-picker-trigger']").text()).toContain("模板提供方");
    expect(wrapper.get("[data-testid='llm-picker-trigger']").text()).toContain("kimi-k2");
  });

  it("syncs the selected agent to the activated thread binding on initial conversation load", async () => {
    const activateChatThread = vi.fn().mockResolvedValue({
      session_id: "session-1",
      thread_id: "thread-1",
      template_id: 9,
      effective_provider_id: 8,
      effective_model: "kimi-k2",
    });

    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session({ id: "session-1", name: "旧会话" })]),
        listChatThreads: vi.fn().mockResolvedValue([thread({ id: "thread-1", title: "旧线程" })]),
        listChatMessages: vi.fn().mockResolvedValue([message("assistant", "旧回复")]),
        activateChatThread,
        listTemplates: vi.fn().mockResolvedValue([
          template({ id: 3, display_name: "通用助手", provider_id: 7, model_id: "glm-4.7" }),
          template({ id: 9, display_name: "代码助手", provider_id: 8, model_id: "kimi-k2" }),
        ]),
        listProviders: vi.fn().mockResolvedValue([
          provider({ id: 7, default_model: "glm-4.7", models: ["glm-4.7"] }),
          provider({ id: 8, default_model: "kimi-default", models: ["kimi-default", "kimi-k2"] }),
        ]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(activateChatThread).toHaveBeenCalledWith("session-1", "thread-1");
    expect(wrapper.get("[data-testid='agent-picker-trigger']").text()).toContain("代码助手");
    expect(wrapper.get("[data-testid='llm-picker-trigger']").text()).toContain("kimi-k2");
  });

  it("activates the requested session and thread from the initial route query", async () => {
    routeState.query = {
      session: "session-2",
      thread: "thread-2",
    };
    const listChatThreads = vi.fn().mockImplementation(async (sessionId: string) => {
      if (sessionId === "session-1") {
        return [thread({ id: "thread-1", title: "一号线程" })];
      }
      if (sessionId === "session-2") {
        return [thread({ id: "thread-2", title: "二号线程" })];
      }
      return [];
    });
    const listChatMessages = vi.fn().mockImplementation(async (sessionId: string, threadId: string) => {
      if (sessionId === "session-2" && threadId === "thread-2") {
        return [message("assistant", "二号回复")];
      }
      return [message("assistant", "一号回复")];
    });

    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([
          session({ id: "session-1", name: "默认会话" }),
          session({ id: "session-2", name: "二号会话" }),
        ]),
        listChatThreads,
        listChatMessages,
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(listChatThreads).toHaveBeenCalledWith("session-2");
    expect(listChatMessages).toHaveBeenLastCalledWith("session-2", "thread-2");
    expect(wrapper.text()).toContain("二号回复");
    expect(wrapper.text()).not.toContain("一号回复");
  });

  it("falls back to the first thread when the requested session is missing", async () => {
    routeState.query = {
      session: "missing-session",
      thread: "thread-2",
    };
    const listChatThreads = vi.fn().mockResolvedValue([
      thread({ id: "thread-1", title: "一号线程" }),
      thread({ id: "thread-2", title: "二号线程" }),
    ]);
    const listChatMessages = vi.fn().mockImplementation(async (_sessionId: string, threadId: string) => {
      if (threadId === "thread-1") {
        return [message("assistant", "一号回复")];
      }
      return [message("assistant", "二号回复")];
    });

    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session({ id: "session-1", name: "默认会话" })]),
        listChatThreads,
        listChatMessages,
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(listChatThreads).toHaveBeenCalledWith("session-1");
    expect(listChatMessages).toHaveBeenLastCalledWith("session-1", "thread-1");
    expect(wrapper.text()).toContain("一号回复");
    expect(wrapper.text()).not.toContain("二号回复");
  });

  it("loads and renders dispatched subagents for the active thread", async () => {
    const listChatThreadJobs = vi.fn().mockResolvedValue([
      dispatchedJob({ job_id: "job-1", title: "整理会议纪要", subagent_name: "researcher" }),
    ]);
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([]),
        listChatThreadJobs,
      }),
    );

    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(listChatThreadJobs).toHaveBeenCalledWith("session-1", "thread-1");
    expect(wrapper.text()).toContain("已派发 subagent");
    expect(wrapper.text()).toContain("整理会议纪要");
    expect(wrapper.text()).toContain("researcher");
  });

  it("hides the dispatched jobs rail after an empty job list loads", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([]),
        listChatThreadJobs: vi.fn().mockResolvedValue([]),
      }),
    );

    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.find(".dispatched-jobs").exists()).toBe(false);
    expect(wrapper.classes()).not.toContain("chat-page--with-dispatched-jobs");
  });

  it("scrolls the primary chat stream to the bottom when messages render", async () => {
    const messagesDeferred = createDeferred<ChatMessageRecord[]>();
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session({ id: "session-1", name: "旧会话" })]),
        listChatThreads: vi.fn().mockResolvedValue([thread({ id: "thread-1", title: "旧线程" })]),
        listChatMessages: vi.fn().mockReturnValue(messagesDeferred.promise),
      }),
    );
    const wrapper = mount(ChatPage);
    const stream = wrapper.get(".chat-page").element as HTMLDivElement;
    const scrollTo = vi.fn();
    Object.defineProperty(stream, "clientHeight", { configurable: true, value: 540 });
    Object.defineProperty(stream, "scrollHeight", { configurable: true, value: 1200 });
    Object.defineProperty(stream, "scrollTop", { configurable: true, writable: true, value: 660 });
    stream.scrollTo = scrollTo;

    messagesDeferred.resolve([
      message("user", "旧消息"),
      message("assistant", "很长的旧回复"),
    ]);
    await flushPromises();

    expect(scrollTo).toHaveBeenCalledWith({ top: 1200, behavior: "auto" });
  });

  it("does not force the primary chat stream down when the user is reading earlier messages", async () => {
    const messagesDeferred = createDeferred<ChatMessageRecord[]>();
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session({ id: "session-1", name: "旧会话" })]),
        listChatThreads: vi.fn().mockResolvedValue([thread({ id: "thread-1", title: "旧线程" })]),
        listChatMessages: vi.fn().mockReturnValue(messagesDeferred.promise),
      }),
    );
    const wrapper = mount(ChatPage);
    const stream = wrapper.get(".chat-page").element as HTMLDivElement;
    const scrollTo = vi.fn();
    Object.defineProperty(stream, "clientHeight", { configurable: true, value: 540 });
    Object.defineProperty(stream, "scrollHeight", { configurable: true, value: 1200 });
    Object.defineProperty(stream, "scrollTop", { configurable: true, writable: true, value: 120 });
    stream.scrollTo = scrollTo;

    await wrapper.get(".chat-page").trigger("scroll");
    messagesDeferred.resolve([
      message("user", "旧消息"),
      message("assistant", "很长的旧回复"),
    ]);
    await flushPromises();

    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("uses one primary chat stream with composer and runtime activity as overlays", () => {
    const source = readFileSync("src/features/chat/ChatPage.vue", "utf8");
    const panelSource = readFileSync("src/features/chat/components/ChatConversationPanel.vue", "utf8");
    const railSource = readFileSync("src/features/chat/components/RuntimeActivityRail.vue", "utf8");

    expect(source).toContain("chat-body-stream");
    expect(source).toContain("chat-runtime-floating-layer");
    expect(source).toContain("chat-page--with-dispatched-jobs");
    expect(source).toContain('v-if="showDispatchedJobsPanel"');
    expect(source).toContain(".chat-page.chat-page--immersive");
    expect(source).toContain("ref=\"chatBodyStreamRef\"");
    expect(source).toContain("@scroll.passive=\"handleChatBodyScroll\"");
    expect(source).toContain(".chat-page {");
    expect(source).toContain("overflow-y: auto;");
    expect(source).toContain(".chat-body-stream {");
    expect(source).toContain("overflow-y: visible;");
    expect(source).not.toContain("scrollbar-width: none;");
    expect(source).not.toContain(".chat-body-stream::-webkit-scrollbar");
    expect(source).toContain("--chat-composer-width: 1120px;");
    expect(source).not.toContain("--chat-message-width:");
    expect(source).toContain("--chat-dock-clearance: 132px;");
    expect(source).toContain("--chat-dock-clearance: 160px;");
    expect(source).toContain(".chat-body-stream::after");
    expect(source).toContain("flex: 0 0 calc(var(--chat-dock-clearance, 132px) + var(--space-6));");
    expect(source).toContain("padding: var(--space-6) var(--space-6) 0;");
    expect(source).toContain("width: min(100%, var(--chat-composer-width));");
    expect(source).not.toContain("calc((100% - var(--chat-message-width)) / 2)");
    expect(source).toContain("padding-right: calc(var(--chat-rail-width) + var(--chat-layout-gap) + var(--space-6));");
    expect(source).toContain(".chat-page--with-dispatched-jobs .chat-runtime-floating-layer");
    expect(source).toContain("position: static;");
    expect(source).toContain("position: absolute;");
    expect(source).toContain("pointer-events: none;");
    expect(source).not.toContain("pointer-events: auto;\n  }\n\n  .chat-runtime-floating-layer :deep(.runtime-rail--collapsed)");
    expect(source).not.toContain("chat-workspace");
    expect(source).not.toContain("chat-main-column");
    expect(source).not.toContain("--chat-sidecar-width");
    expect(source).not.toContain("grid-template-columns: minmax(0, 1fr) minmax(0, var(--chat-message-width)) minmax(0, 1fr);");

    expect(panelSource).toContain("min-height: 100%;");
    expect(panelSource).not.toMatch(/(^|\n)\s*height: 100%;/);

    expect(railSource).toContain("grid-template-rows: auto minmax(0, 1fr);");
    expect(railSource).toContain(".runtime-rail__list");
    expect(railSource).toContain("overflow: auto;");
    expect(railSource).not.toContain("position: sticky;");
  });

  it("keeps the temporary missing job route quiet when opening a dispatched job", async () => {
    routerPush.mockImplementationOnce(() => {
      throw new Error('No match for {"name":"chat-job"}');
    });
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([]),
        listChatThreadJobs: vi.fn().mockResolvedValue([dispatchedJob()]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    await wrapper.get("[data-testid='dispatched-job-job-1']").trigger("click");
    await flushPromises();

    expect(consoleError).not.toHaveBeenCalled();
    consoleError.mockRestore();
  });

  it("logs unexpected dispatched job navigation failures", async () => {
    routerPush.mockRejectedValueOnce(new Error("navigation exploded"));
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([]),
        listChatThreadJobs: vi.fn().mockResolvedValue([dispatchedJob()]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    await wrapper.get("[data-testid='dispatched-job-job-1']").trigger("click");
    await flushPromises();

    expect(consoleError).toHaveBeenCalledWith("Failed to open dispatched job", expect.any(Error));
    consoleError.mockRestore();
  });

  it("creates a new session and shows a notice when switching agents from an existing conversation", async () => {
    const createChatSessionWithThread = vi.fn().mockResolvedValue({
      session_key: "session-2",
      session_id: "session-2",
      template_id: 9,
      thread_id: "thread-2",
      effective_provider_id: 8,
      effective_model: "kimi-k2",
    } satisfies ChatSessionPayload);
    const listChatSessions = vi
      .fn()
      .mockResolvedValueOnce([session({ id: "session-1", name: "旧会话" })])
      .mockResolvedValueOnce([
        session({ id: "session-2", name: "新的 Web 对话" }),
        session({ id: "session-1", name: "旧会话" }),
      ]);
    const listChatThreads = vi.fn().mockImplementation(async (sessionId: string) => {
      if (sessionId === "session-2") return [thread({ id: "thread-2", title: null })];
      return [thread({ id: "thread-1", title: "旧线程" })];
    });
    const listChatMessages = vi.fn().mockImplementation(async (sessionId: string, threadId: string) => {
      if (sessionId === "session-1" && threadId === "thread-1") {
        return [message("user", "旧消息"), message("assistant", "旧回复")];
      }
      return [];
    });

    setApiClient(
      makeApiClient({
        listChatSessions,
        listChatThreads,
        listChatMessages,
        createChatSessionWithThread,
        listTemplates: vi.fn().mockResolvedValue([
          template({ id: 3, display_name: "通用助手", provider_id: 7, model_id: "glm-4.7" }),
          template({ id: 9, display_name: "代码助手", provider_id: 8, model_id: "kimi-k2" }),
        ]),
        listProviders: vi.fn().mockResolvedValue([
          provider({ id: 7, default_model: "glm-4.7", models: ["glm-4.7"] }),
          provider({ id: 8, default_model: "kimi-default", models: ["kimi-default", "kimi-k2"] }),
        ]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.text()).toContain("旧回复");

    await chooseAgent(wrapper, 9);

    expect(createChatSessionWithThread).toHaveBeenCalledWith({
      name: "新的 Web 对话",
      template_id: 9,
      provider_id: 8,
      model: "kimi-k2",
    });
    expect(wrapper.text()).toContain("已切换到「代码助手」，新的消息将在新会话中发送。");
    expect(wrapper.text()).not.toContain("旧回复");
  });

  it("only syncs provider and model when switching agents from an empty draft", async () => {
    const createChatSessionWithThread = vi.fn();
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([]),
        createChatSessionWithThread,
        listTemplates: vi.fn().mockResolvedValue([
          template({ id: 3, display_name: "通用助手", provider_id: 7, model_id: "glm-4.7" }),
          template({ id: 9, display_name: "代码助手", provider_id: 8, model_id: "kimi-k2" }),
        ]),
        listProviders: vi.fn().mockResolvedValue([
          provider({ id: 7, default_model: "glm-4.7", models: ["glm-4.7"] }),
          provider({ id: 8, default_model: "kimi-default", models: ["kimi-default", "kimi-k2"] }),
        ]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    await chooseAgent(wrapper, 9);

    expect(createChatSessionWithThread).not.toHaveBeenCalled();
    expect(wrapper.get("[data-testid='agent-picker-trigger']").text()).toContain("代码助手");
    expect(wrapper.get("[data-testid='llm-picker-trigger']").text()).toContain("kimi-k2");
  });

  it("keeps the existing conversation when switching agents fails to create the new session", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session({ id: "session-1", name: "旧会话" })]),
        listChatThreads: vi.fn().mockResolvedValue([thread({ id: "thread-1", title: "旧线程" })]),
        listChatMessages: vi.fn().mockResolvedValue([message("assistant", "旧回复")]),
        createChatSessionWithThread: vi.fn().mockRejectedValue(new Error("create failed")),
        listTemplates: vi.fn().mockResolvedValue([
          template({ id: 3, display_name: "通用助手", provider_id: 7, model_id: "glm-4.7" }),
          template({ id: 9, display_name: "代码助手", provider_id: 8, model_id: "kimi-k2" }),
        ]),
        listProviders: vi.fn().mockResolvedValue([
          provider({ id: 7, default_model: "glm-4.7", models: ["glm-4.7"] }),
          provider({ id: 8, default_model: "kimi-default", models: ["kimi-default", "kimi-k2"] }),
        ]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    await chooseAgent(wrapper, 9);

    expect(wrapper.text()).toContain("旧回复");
    expect(wrapper.text()).toContain("create failed");
    expect(wrapper.get("[data-testid='agent-picker-trigger']").text()).toContain("通用助手");
    expect(wrapper.get("[data-testid='llm-picker-trigger']").text()).toContain("glm-4.7");
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

  it("closes history dialog after selecting a thread", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread({ id: "thread-2", title: "二号线程" })]),
        listChatMessages: vi.fn().mockResolvedValue([]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    const historyBtn = wrapper.findAll("button").find((b) => b.text().includes("历史"));
    await historyBtn!.trigger("click");
    await flushPromises();

    const threadButton = Array.from(document.querySelectorAll(".history-dialog__session-item")).find((item) =>
      item.textContent?.includes("二号线程"),
    ) as HTMLElement | undefined;
    expect(threadButton).toBeDefined();
    threadButton!.click();
    await flushPromises();

    expect(document.querySelector(".history-dialog")).toBeNull();
  });

  it("shows an error state when chat sessions fail to load", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockRejectedValue(new Error("Request failed: 502")),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.text()).toContain("对话会话加载失败：Request failed: 502");
  });

  it("shows chat load errors directly above the composer in a red alert", async () => {
    setApiClient(
      makeApiClient({
        getChatOptions: vi.fn().mockRejectedValue(new Error("Request failed: 500")),
        listChatSessions: vi.fn().mockRejectedValue(new Error("Request failed: 500")),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    const composerShell = wrapper.get(".chat-page__composer-shell");
    const children = Array.from(composerShell.element.children).map((element) => element.className);
    const error = composerShell.get(".chat-page__composer-error");

    expect(children[0]).toContain("chat-page__composer-error");
    expect(children[1]).toContain("composer-bar");
    expect(error.text()).toBe("对话配置加载失败：Request failed: 500；对话会话加载失败：Request failed: 500");
    expect(error.attributes("role")).toBe("alert");
    expect(error.classes()).toContain("error-message");

    const source = readFileSync("src/features/chat/ChatPage.vue", "utf8");
    expect(source).toContain("color: var(--danger);");
    expect(source).toContain("background: var(--danger-bg);");
    expect(source).toContain("border: 1px solid var(--danger-border);");
    expect(source).not.toContain("var(--status-danger");
  });

  it("switches to the selected session thread from history and refreshes its messages", async () => {
    const listChatThreads = vi.fn().mockImplementation(async (sessionId: string) => {
      if (sessionId === "session-1") {
        return [thread({ id: "thread-1", title: "一号线程" })];
      }
      return [thread({ id: "thread-2", title: "二号线程" })];
    });
    const listChatMessages = vi.fn().mockImplementation(async (sessionId: string, threadId: string) => {
      if (sessionId === "session-1" && threadId === "thread-1") {
        return [message("assistant", "一号回复")];
      }
      if (sessionId === "session-2" && threadId === "thread-2") {
        return [message("assistant", "二号回复")];
      }
      return [];
    });

    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([
          session({ id: "session-1", name: "默认会话" }),
          session({ id: "session-2", name: "二号会话" }),
        ]),
        listChatThreads,
        listChatMessages,
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    const historyBtn = wrapper.findAll("button").find((b) => b.text().includes("历史"));
    await historyBtn!.trigger("click");
    await flushPromises();

    const secondSession = Array.from(document.querySelectorAll(".history-dialog__session-item")).find((item) =>
      item.textContent?.includes("二号会话"),
    ) as HTMLElement | undefined;
    expect(secondSession).toBeDefined();
    secondSession!.click();
    await flushPromises();

    const secondThread = Array.from(document.querySelectorAll(".history-dialog__session-item")).find((item) =>
      item.textContent?.includes("二号线程"),
    ) as HTMLElement | undefined;
    expect(secondThread).toBeDefined();
    secondThread!.click();
    await flushPromises();

    expect(wrapper.text()).toContain("二号回复");
    expect(listChatMessages).toHaveBeenLastCalledWith("session-2", "thread-2");
  });

  it("selects the first remaining thread after deleting the active session", async () => {
    const listChatSessions = vi
      .fn()
      .mockResolvedValueOnce([
        session({ id: "session-1", name: "默认会话" }),
        session({ id: "session-2", name: "二号会话" }),
      ])
      .mockResolvedValueOnce([session({ id: "session-2", name: "二号会话" })]);
    const listChatThreads = vi.fn().mockImplementation(async (sessionId: string) => {
      if (sessionId === "session-1") {
        return [thread({ id: "thread-1", title: "一号线程" })];
      }
      if (sessionId === "session-2") {
        return [thread({ id: "thread-2", title: "二号线程" })];
      }
      return [];
    });
    const listChatMessages = vi.fn().mockImplementation(async (sessionId: string, threadId: string) => {
      if (sessionId === "session-1" && threadId === "thread-1") {
        return [message("assistant", "一号回复")];
      }
      if (sessionId === "session-2" && threadId === "thread-2") {
        return [message("assistant", "二号回复")];
      }
      return [];
    });

    const deleteChatSession = vi.fn().mockResolvedValue({ deleted: true });
    setApiClient(
      makeApiClient({
        listChatSessions,
        deleteChatSession,
        listChatThreads,
        listChatMessages,
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    const historyBtn = wrapper.findAll("button").find((b) => b.text().includes("历史"));
    await historyBtn!.trigger("click");
    await flushPromises();

    const deleteButtons = Array.from(document.querySelectorAll(".history-dialog__action-btn--danger")) as HTMLElement[];
    expect(deleteButtons.length).toBeGreaterThan(0);
    deleteButtons[0].click();
    await flushPromises();

    const confirmDelete = Array.from(document.querySelectorAll(".history-dialog button")).find((item) =>
      item.textContent?.includes("删除"),
    ) as HTMLElement | undefined;
    expect(confirmDelete).toBeDefined();
    confirmDelete!.click();
    await flushPromises();

    expect(deleteChatSession).toHaveBeenCalledWith("session-1");
    expect(wrapper.text()).toContain("二号回复");
    expect(listChatMessages).toHaveBeenLastCalledWith("session-2", "thread-2");
  });

  it("keeps polling after stream settles until a new assistant reply appears", async () => {
    let handlers: ChatThreadEventHandlers | undefined;
    const oldAssistant = message("assistant", "旧回复");
    const newAssistant = message("assistant", "新回复");
    const listChatMessages = vi
      .fn()
      .mockResolvedValueOnce([oldAssistant])
      .mockResolvedValueOnce([oldAssistant])
      .mockResolvedValueOnce([oldAssistant, newAssistant]);

    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages,
        subscribeChatThread: vi.fn((_sessionId, _threadId, nextHandlers) => {
          handlers = nextHandlers;
          return { close: vi.fn() } as RuntimeEventSubscription;
        }),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    await wrapper.get("[data-testid='chat-input']").setValue("继续");
    await wrapper.get("[data-testid='chat-input']").trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(handlers).toBeDefined();
    handlers!.onEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: { type: "turn_settled" },
    });
    await flushPromises();
    await flushPromises();

    expect(wrapper.text()).toContain("新回复");
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

  it("does not render the old conversation header even when a thread is active", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread({ title: "产品讨论" })]),
        listChatMessages: vi.fn().mockResolvedValue([message("assistant", "欢迎继续")]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.text()).toContain("欢迎继续");
    expect(wrapper.text()).not.toContain("Conversation");
    expect(wrapper.find(".chat-panel__header").exists()).toBe(false);
  });

  it("wraps the chat stage in a TinyRobot bubble provider for markdown rendering", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([
          message("assistant", "# 标题", {
            reasoning_content: "先整理问题，再输出 markdown 答案。",
          }),
        ]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    const provider = wrapper.find(".tr-bubble-provider-stub");
    expect(provider.exists()).toBe(true);
    expect(provider.attributes("data-fallback-content-renderer")).toBe("set");

    const assistantBubble = wrapper.find(".tr-bubble-stub[data-role='assistant']");
    expect(assistantBubble.attributes("data-reasoning")).toContain("先整理问题");
  });

  it("uses the immersive chat layout without the conversation header chrome", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([]),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    expect(wrapper.find(".chat-page--immersive").exists()).toBe(true);
    expect(wrapper.find(".chat-page--single-scroll").exists()).toBe(true);
    expect(wrapper.find(".composer-bar--dock").exists()).toBe(true);
    expect(wrapper.find(".chat-panel__header").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("Conversation");
    expect(wrapper.text()).not.toContain("激活");
    expect(wrapper.text()).not.toContain("取消运行");
  });

  it("does not render the streaming submission banner after sending a message", async () => {
    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([]),
        subscribeChatThread: vi.fn().mockReturnValue({ close: vi.fn() }),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    await wrapper.get("[data-testid='chat-input']").setValue("继续");
    await wrapper.get("[data-testid='chat-input']").trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(wrapper.text()).not.toContain("消息已提交，正在等待流式结果。");
  });

  it("routes runtime tool activity into the right rail instead of the assistant message", async () => {
    let handlers: ChatThreadEventHandlers | undefined;

    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([]),
        subscribeChatThread: vi.fn((_sessionId, _threadId, nextHandlers) => {
          handlers = nextHandlers;
          return { close: vi.fn() } as RuntimeEventSubscription;
        }),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    await wrapper.get("[data-testid='chat-input']").setValue("继续");
    await wrapper.get("[data-testid='chat-input']").trigger("keydown", { key: "Enter" });
    await flushPromises();

    handlers!.onEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "tool_started",
        tool_call_id: "call-shell",
        tool_name: "shell",
        arguments: { cmd: "pwd" },
      },
    });
    await flushPromises();

    expect(wrapper.text()).not.toContain("本轮运行活动");
    expect(wrapper.find(".runtime-rail").exists()).toBe(true);
    expect(wrapper.find(".runtime-rail").text()).toContain("shell");

    handlers!.onEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "tool_completed",
        tool_call_id: "call-shell",
        tool_name: "shell",
        result: "/workspace/project",
        is_error: false,
      },
    });
    await flushPromises();

    const panel = wrapper.getComponent(ChatConversationPanel);
    const messages = panel.props("robotMessages") as Array<{
      id?: string;
      role: string;
      state?: { toolDetails?: unknown[] };
    }>;
    const pendingAssistant = messages.find((item) => item.id === "pending-assistant");
    expect(pendingAssistant?.role).toBe("assistant");
    expect(pendingAssistant?.state?.toolDetails).toBeUndefined();
    expect(wrapper.find(".runtime-rail").text()).toContain("完成");
  });

  it("collapses and expands the floating runtime activity list without removing its summary", async () => {
    let handlers: ChatThreadEventHandlers | undefined;

    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([]),
        subscribeChatThread: vi.fn((_sessionId, _threadId, nextHandlers) => {
          handlers = nextHandlers;
          return { close: vi.fn() } as RuntimeEventSubscription;
        }),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    await wrapper.get("[data-testid='chat-input']").setValue("继续");
    await wrapper.get("[data-testid='chat-input']").trigger("keydown", { key: "Enter" });
    await flushPromises();

    handlers!.onEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "tool_started",
        tool_call_id: "call-shell",
        tool_name: "shell",
        arguments: { cmd: "pwd" },
      },
    });
    await flushPromises();

    const toggle = wrapper.get(".runtime-rail__toggle");
    expect(toggle.attributes("aria-expanded")).toBe("true");
    expect(wrapper.findAll(".runtime-rail__item")).toHaveLength(1);

    await toggle.trigger("click");
    await flushPromises();

    expect(wrapper.get(".runtime-rail").classes()).toContain("runtime-rail--collapsed");
    expect(wrapper.get(".runtime-rail__toggle").attributes("aria-expanded")).toBe("false");
    expect(wrapper.get(".runtime-rail__list").attributes("style")).toContain("display: none");
    expect(wrapper.get(".runtime-rail").text()).toContain("当前运行");
    expect(wrapper.get(".runtime-rail").text()).toContain("1");

    await wrapper.get(".runtime-rail__toggle").trigger("click");
    await flushPromises();

    expect(wrapper.get(".runtime-rail__toggle").attributes("aria-expanded")).toBe("true");
    expect(wrapper.get(".runtime-rail__list").attributes("style") ?? "").not.toContain("display: none");
    expect(wrapper.findAll(".runtime-rail__item")).toHaveLength(1);
  });

  it("keeps runtime activity ordered from first started to latest while updating in place", async () => {
    let handlers: ChatThreadEventHandlers | undefined;

    setApiClient(
      makeApiClient({
        listChatSessions: vi.fn().mockResolvedValue([session()]),
        listChatThreads: vi.fn().mockResolvedValue([thread()]),
        listChatMessages: vi.fn().mockResolvedValue([]),
        subscribeChatThread: vi.fn((_sessionId, _threadId, nextHandlers) => {
          handlers = nextHandlers;
          return { close: vi.fn() } as RuntimeEventSubscription;
        }),
      }),
    );
    const wrapper = mount(ChatPage);
    await flushPromises();

    await wrapper.get("[data-testid='chat-input']").setValue("继续");
    await wrapper.get("[data-testid='chat-input']").trigger("keydown", { key: "Enter" });
    await flushPromises();

    handlers!.onEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "tool_started",
        tool_call_id: "call-shell",
        tool_name: "shell",
        arguments: { cmd: "pwd" },
      },
    });
    handlers!.onEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "job_dispatched",
        job_id: "job-2",
        prompt: "后台分析",
      },
    });
    handlers!.onEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "tool_completed",
        tool_call_id: "call-shell",
        tool_name: "shell",
        result: "/workspace/project",
        is_error: false,
      },
    });
    await flushPromises();

    const rows = wrapper.findAll(".runtime-rail__item");
    expect(rows).toHaveLength(2);
    expect(rows[0]!.text()).toContain("shell");
    expect(rows[0]!.text()).toContain("完成");
    expect(rows[1]!.text()).toContain("后台 Job job-2");
    expect(rows[1]!.text()).toContain("运行中");
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

    // Should show the existing conversation content before resetting
    expect(wrapper.text()).toContain("旧消息");
    expect(wrapper.text()).toContain("旧回复");

    // Click new chat button
    const newChatBtn = wrapper.findAll("button").find((b) => b.text().includes("新对话"));
    expect(newChatBtn).toBeDefined();
    await newChatBtn!.trigger("click");
    await flushPromises();

    // Should show prompt panel (no active thread)
    expect(wrapper.find(".prompt-panel").exists()).toBe(true);
  });
});
