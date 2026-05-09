import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import SchedulerPage from "./SchedulerPage.vue";
import {
  resetApiClient,
  setApiClient,
  type ApiClient,
  type CreateScheduledMessageRequest,
  type ScheduledMessageSummary,
} from "@/lib/api";

function schedule(overrides: Partial<ScheduledMessageSummary> = {}): ScheduledMessageSummary {
  return {
    id: "schedule-1",
    name: "每日检查",
    status: "pending",
    session_id: "session-1",
    thread_id: "thread-1",
    prompt: "Run daily check",
    cron_expr: "0 9 * * *",
    scheduled_at: "2026-05-10T01:00:00Z",
    timezone: "Asia/Shanghai",
    last_error: null,
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
    resetApiClient();
  });

  it("loads and renders scheduled messages", async () => {
    const listScheduledMessages = vi.fn(async () => [
      schedule(),
      schedule({
        id: "schedule-2",
        name: "失败任务",
        status: "failed",
        cron_expr: null,
        timezone: null,
        last_error: "dispatch failed",
      }),
    ]);
    setApiClient(makeApiClient({ listScheduledMessages }));

    const wrapper = mount(SchedulerPage);
    await flushPromises();

    expect(listScheduledMessages).toHaveBeenCalledTimes(1);
    expect(wrapper.text()).toContain("Scheduler");
    expect(wrapper.text()).toContain("总任务数");
    expect(wrapper.text()).toContain("每日检查");
    expect(wrapper.text()).toContain("失败任务");
    expect(wrapper.text()).toContain("dispatch failed");
  });

  it("creates a cron scheduled message", async () => {
    const listScheduledMessages = vi.fn(async () => []);
    const createScheduledMessage = vi.fn(async (_input: CreateScheduledMessageRequest) => schedule());
    setApiClient(makeApiClient({ createScheduledMessage, listScheduledMessages }));

    const wrapper = mount(SchedulerPage);
    await flushPromises();

    await wrapper.get('[data-testid="schedule-session-id"]').setValue("session-9");
    await wrapper.get('[data-testid="schedule-thread-id"]').setValue("thread-9");
    await wrapper.get('[data-testid="schedule-name"]').setValue("晨间检查");
    await wrapper.get('[data-testid="schedule-prompt"]').setValue("检查今天的状态");
    await wrapper.get('[data-testid="schedule-cron-expr"]').setValue("30 8 * * *");
    await wrapper.get('[data-testid="schedule-timezone"]').setValue("Asia/Shanghai");
    await wrapper.get('[data-testid="create-schedule"]').trigger("click");
    await flushPromises();

    expect(createScheduledMessage).toHaveBeenCalledWith({
      session_id: "session-9",
      thread_id: "thread-9",
      name: "晨间检查",
      prompt: "检查今天的状态",
      cron_expr: "30 8 * * *",
      timezone: "Asia/Shanghai",
    });
    expect(createScheduledMessage.mock.calls[0][0]).not.toHaveProperty("scheduled_at");
    expect(wrapper.text()).toContain("定时任务已创建。");
    expect(listScheduledMessages).toHaveBeenCalledTimes(2);
  });

  it("creates a one-shot scheduled message", async () => {
    const listScheduledMessages = vi.fn(async () => []);
    const createScheduledMessage = vi.fn(async (_input: CreateScheduledMessageRequest) => schedule({ cron_expr: null }));
    setApiClient(makeApiClient({ createScheduledMessage, listScheduledMessages }));

    const wrapper = mount(SchedulerPage);
    await flushPromises();

    await wrapper.get('[data-testid="schedule-mode"]').setValue("once");
    await wrapper.get('[data-testid="schedule-session-id"]').setValue("session-10");
    await wrapper.get('[data-testid="schedule-thread-id"]').setValue("thread-10");
    await wrapper.get('[data-testid="schedule-prompt"]').setValue("只运行一次");
    await wrapper.get('[data-testid="schedule-scheduled-at"]').setValue("2026-05-10T01:00:00Z");
    await wrapper.get('[data-testid="create-schedule"]').trigger("click");
    await flushPromises();

    expect(createScheduledMessage).toHaveBeenCalledWith({
      session_id: "session-10",
      thread_id: "thread-10",
      name: "Scheduled message",
      prompt: "只运行一次",
      scheduled_at: "2026-05-10T01:00:00Z",
    });
    expect(createScheduledMessage.mock.calls[0][0]).not.toHaveProperty("cron_expr");
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
    setApiClient(makeApiClient());

    const wrapper = mount(SchedulerPage);
    await flushPromises();

    expect(wrapper.text()).toContain("当前 API 客户端不支持 Scheduler。");
  });
});
