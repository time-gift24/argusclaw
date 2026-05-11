import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { reactive } from "vue";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));
vi.mock("@opentiny/tiny-robot", async () => import("@/test/stubs/tiny-robot"));

const routeState = reactive({
  params: {
    scheduleId: "schedule-1",
    sessionId: "session-1",
    threadId: "thread-1",
  },
});

vi.mock("vue-router", () => ({
  useRoute: () => routeState,
}));

import SchedulerRunPage from "./SchedulerRunPage.vue";
import {
  resetApiClient,
  setApiClient,
  type ApiClient,
  type ChatMessageRecord,
  type ScheduledMessageSummary,
} from "@/lib/api";

function schedule(overrides: Partial<ScheduledMessageSummary> = {}): ScheduledMessageSummary {
  return {
    id: "schedule-1",
    name: "每日检查",
    status: "pending",
    template_id: 7,
    provider_id: 3,
    model: "alpha",
    last_session_id: "session-1",
    last_thread_id: "thread-1",
    run_history: [
      {
        session_id: "session-1",
        thread_id: "thread-1",
        created_at: "2026-05-10T01:00:00Z",
      },
    ],
    prompt: "Run daily check",
    cron_expr: "0 9 * * *",
    scheduled_at: null,
    timezone: "Asia/Shanghai",
    last_error: null,
    ...overrides,
  };
}

function message(role: ChatMessageRecord["role"], content: string): ChatMessageRecord {
  return {
    role,
    content,
    reasoning_content: null,
    content_parts: [],
    tool_call_id: null,
    name: null,
    tool_calls: null,
    metadata: null,
  };
}

function makeApiClient(overrides: Partial<ApiClient> = {}): ApiClient {
  return {
    getHealth: async () => ({ status: "ok" }),
    getBootstrap: async () => ({
      instance_name: "",
      provider_count: 0,
      template_count: 0,
      mcp_server_count: 0,
      default_provider_id: null,
      default_template_id: null,
      mcp_ready_count: 0,
    }),
    listProviders: async () => [],
    listTemplates: async () => [],
    saveProvider: async (input) => input,
    saveTemplate: async (input) => input,
    listMcpServers: async () => [],
    saveMcpServer: async (input) => input,
    ...overrides,
  } as ApiClient;
}

describe("SchedulerRunPage", () => {
  afterEach(() => {
    routeState.params = {
      scheduleId: "schedule-1",
      sessionId: "session-1",
      threadId: "thread-1",
    };
    resetApiClient();
  });

  it("loads the scheduler run messages without rendering the chat composer", async () => {
    const listScheduledMessages = vi.fn(async () => [schedule()]);
    const listChatMessages = vi.fn(async () => [
      message("user", "scheduled prompt"),
      message("assistant", "scheduled result"),
    ]);
    setApiClient(makeApiClient({ listChatMessages, listScheduledMessages }));

    const wrapper = mount(SchedulerRunPage);
    await flushPromises();

    expect(listScheduledMessages).toHaveBeenCalledTimes(1);
    expect(listChatMessages).toHaveBeenCalledWith("session-1", "thread-1");
    expect(wrapper.text()).toContain("定时任务运行记录");
    expect(wrapper.text()).toContain("每日检查");
    expect(wrapper.text()).toContain("scheduled prompt");
    expect(wrapper.text()).toContain("scheduled result");
    expect(wrapper.find("textarea").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("新的 Web 对话");
  });

  it("shows a Chinese notice when the run is missing", async () => {
    const listChatMessages = vi.fn(async () => []);
    setApiClient(makeApiClient({
      listChatMessages,
      listScheduledMessages: async () => [schedule({ run_history: [] })],
    }));

    const wrapper = mount(SchedulerRunPage);
    await flushPromises();

    expect(listChatMessages).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("未找到这次 Scheduler 运行记录");
  });
});
