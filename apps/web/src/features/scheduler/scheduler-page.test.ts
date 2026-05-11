import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

const push = vi.fn();

vi.mock("vue-router", () => ({
  useRouter: () => ({ push }),
}));

import SchedulerPage from "./SchedulerPage.vue";
import {
  resetApiClient,
  setApiClient,
  type ApiClient,
  type AgentRecord,
  type LlmProviderRecord,
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
    scheduled_at: "2026-05-10T01:00:00Z",
    timezone: "Asia/Shanghai",
    last_error: null,
    ...overrides,
  };
}

function provider(overrides: Partial<LlmProviderRecord> = {}): LlmProviderRecord {
  return {
    id: 3,
    kind: "openai-compatible",
    display_name: "测试 Provider",
    base_url: "https://example.invalid/v1",
    api_key: "",
    models: ["alpha", "beta"],
    model_config: {},
    default_model: "alpha",
    is_default: true,
    extra_headers: {},
    secret_status: "ready",
    meta_data: {},
    ...overrides,
  };
}

function template(overrides: Partial<AgentRecord> = {}): AgentRecord {
  return {
    id: 7,
    display_name: "巡检 Agent",
    description: "",
    version: "1.0.0",
    provider_id: 3,
    model_id: "alpha",
    system_prompt: "",
    tool_names: [],
    subagent_names: [],
    ...overrides,
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
    getRuntimeState: async () => ({
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
          captured_at: "2026-05-09T00:00:00Z",
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
          captured_at: "2026-05-09T00:00:00Z",
        },
        runtimes: [],
      },
    }),
    listProviders: async () => [],
    getChatOptions: async () => ({
      providers: [provider()],
      templates: [template()],
    }),
    saveProvider: async (input) => input,
    listTemplates: async () => [],
    saveTemplate: async (input) => input,
    listMcpServers: async () => [],
    saveMcpServer: async (input) => input,
    ...overrides,
  } as ApiClient;
}

describe("SchedulerPage", () => {
  afterEach(() => {
    push.mockReset();
    resetApiClient();
  });

  it("loads and renders scheduled messages", async () => {
    const listScheduledMessages = vi.fn(async () => [
      schedule({
        run_history: [
          {
            session_id: "session-1",
            thread_id: "thread-1",
            created_at: "2026-05-10T01:00:00Z",
          },
          {
            session_id: "session-2",
            thread_id: "thread-2",
            created_at: "2026-05-10T00:00:00Z",
          },
          {
            session_id: "session-3",
            thread_id: "thread-3",
            created_at: "2026-05-09T23:00:00Z",
          },
          {
            session_id: "session-4",
            thread_id: "thread-4",
            created_at: "2026-05-09T22:00:00Z",
          },
        ],
      }),
      schedule({
        id: "schedule-2",
        name: "失败任务",
        status: "failed",
        cron_expr: null,
        timezone: null,
        last_error: "dispatch failed",
      }),
    ]);
    const getChatOptions = vi.fn(async () => ({
      providers: [provider()],
      templates: [template()],
    }));
    setApiClient(makeApiClient({ getChatOptions, listScheduledMessages }));

    const wrapper = mount(SchedulerPage);
    await flushPromises();

    expect(listScheduledMessages).toHaveBeenCalledTimes(1);
    expect(getChatOptions).toHaveBeenCalledTimes(1);
    expect(wrapper.text()).toContain("定时任务");
    expect(wrapper.text()).toContain("总任务数");
    expect(wrapper.text()).toContain("每日检查");
    expect(wrapper.text()).toContain("失败任务");
    expect(wrapper.text()).toContain("dispatch failed");
    const historyLink = wrapper.get('a[href="/scheduler/schedule-1/runs/session-1/thread-1"]');
    expect(historyLink.text()).toContain("最近");
    expect(wrapper.find('a[href="/scheduler/schedule-1/runs/session-4/thread-4"]').exists()).toBe(false);
    const createButton = wrapper.get('[data-testid="create-schedule-link"]');
    expect(createButton.text()).toContain("创建任务");
    await createButton.trigger("click");
    expect(push).toHaveBeenCalledWith("/scheduler/new");
    expect(wrapper.get('a[href="/scheduler/schedule-1/edit"]').text()).toContain("编辑");
  });

  it("pauses, triggers, deletes, and refreshes schedules", async () => {
    const listScheduledMessages = vi.fn(async () => [schedule()]);
    const pauseScheduledMessage = vi.fn(async () => schedule({ status: "paused" }));
    const triggerScheduledMessage = vi.fn(async () => schedule({ status: "running" }));
    const deleteScheduledMessage = vi.fn(async () => ({ deleted: true }));
    setApiClient(makeApiClient({
      deleteScheduledMessage,
      listScheduledMessages,
      pauseScheduledMessage,
      triggerScheduledMessage,
    }));

    const wrapper = mount(SchedulerPage);
    await flushPromises();

    await wrapper.get('[data-testid="pause-schedule-schedule-1"]').trigger("click");
    await flushPromises();
    await wrapper.get('[data-testid="trigger-schedule-schedule-1"]').trigger("click");
    await flushPromises();
    await wrapper.get('[data-testid="delete-schedule-schedule-1"]').trigger("click");
    await flushPromises();

    expect(pauseScheduledMessage).toHaveBeenCalledWith("schedule-1");
    expect(triggerScheduledMessage).toHaveBeenCalledWith("schedule-1");
    expect(deleteScheduledMessage).toHaveBeenCalledWith("schedule-1");
    expect(listScheduledMessages).toHaveBeenCalledTimes(4);
  });

  it("shows an error when the API client does not support scheduler operations", async () => {
    setApiClient(makeApiClient({ listScheduledMessages: undefined, getChatOptions: undefined }));

    const wrapper = mount(SchedulerPage);
    await flushPromises();

    expect(wrapper.text()).toContain("当前 API 客户端不支持 Scheduler。");
  });

  it("keeps the chat options error visible when schedule loading succeeds", async () => {
    setApiClient(makeApiClient({
      getChatOptions: vi.fn(async () => {
        throw new Error("加载 Agent 配置失败。");
      }),
      listScheduledMessages: vi.fn(async () => []),
    }));

    const wrapper = mount(SchedulerPage);
    await flushPromises();

    expect(wrapper.text()).toContain("加载 Agent 配置失败。");
  });
});
