import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { reactive } from "vue";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

const push = vi.fn();
const routeState = reactive({
  params: {} as Record<string, string>,
});

vi.mock("vue-router", () => ({
  useRoute: () => routeState,
  useRouter: () => ({ push }),
}));

import SchedulerFormPage from "./SchedulerFormPage.vue";
import {
  resetApiClient,
  setApiClient,
  type AgentRecord,
  type ApiClient,
  type CreateScheduledMessageRequest,
  type LlmProviderRecord,
  type ScheduledMessageSummary,
} from "@/lib/api";

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

function schedule(overrides: Partial<ScheduledMessageSummary> = {}): ScheduledMessageSummary {
  return {
    id: "schedule-1",
    name: "每日检查",
    status: "pending",
    template_id: 7,
    provider_id: 3,
    model: "alpha",
    last_session_id: null,
    last_thread_id: null,
    run_history: [],
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
    getChatOptions: async () => ({
      providers: [provider()],
      templates: [template()],
    }),
    ...overrides,
  } as ApiClient;
}

describe("SchedulerFormPage", () => {
  afterEach(() => {
    routeState.params = {};
    push.mockReset();
    resetApiClient();
  });

  it("creates a cron scheduled message from the child route", async () => {
    const createScheduledMessage = vi.fn(async (_input: CreateScheduledMessageRequest) => schedule());
    setApiClient(makeApiClient({ createScheduledMessage }));

    const wrapper = mount(SchedulerFormPage);
    await flushPromises();

    expect(wrapper.text()).toContain("创建定时任务");
    expect(wrapper.find('[data-testid="schedule-session-id"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="schedule-thread-id"]').exists()).toBe(false);
    await wrapper.get('[data-testid="schedule-name"]').setValue("晨间检查");
    await wrapper.get('[data-testid="schedule-prompt"]').setValue("检查今天的状态");
    await wrapper.get('[data-testid="schedule-cron-expr"]').setValue("30 8 * * *");
    await wrapper.get('[data-testid="schedule-timezone"]').setValue("Asia/Shanghai");
    await wrapper.get('[data-testid="submit-schedule"]').trigger("click");
    await flushPromises();

    expect(createScheduledMessage).toHaveBeenCalledWith({
      template_id: 7,
      provider_id: 3,
      model: "alpha",
      name: "晨间检查",
      prompt: "检查今天的状态",
      cron_expr: "30 8 * * *",
      timezone: "Asia/Shanghai",
    });
    expect(createScheduledMessage.mock.calls[0][0]).not.toHaveProperty("session_id");
    expect(createScheduledMessage.mock.calls[0][0]).not.toHaveProperty("thread_id");
    expect(push).toHaveBeenCalledWith("/scheduler");
  });

  it("loads an existing schedule and saves edits", async () => {
    routeState.params = { scheduleId: "schedule-1" };
    const listScheduledMessages = vi.fn(async () => [schedule()]);
    const updateScheduledMessage = vi.fn(async (_id: string, input: CreateScheduledMessageRequest) =>
      schedule({ ...input, id: "schedule-1" }),
    );
    setApiClient(makeApiClient({ listScheduledMessages, updateScheduledMessage }));

    const wrapper = mount(SchedulerFormPage);
    await flushPromises();

    expect(wrapper.text()).toContain("编辑定时任务");
    expect(wrapper.get<HTMLInputElement>('[data-testid="schedule-name"]').element.value).toBe("每日检查");
    await wrapper.get('[data-testid="schedule-name"]').setValue("晚间检查");
    await wrapper.get('[data-testid="submit-schedule"]').trigger("click");
    await flushPromises();

    expect(updateScheduledMessage).toHaveBeenCalledWith("schedule-1", {
      template_id: 7,
      provider_id: 3,
      model: "alpha",
      name: "晚间检查",
      prompt: "Run daily check",
      cron_expr: "0 9 * * *",
      timezone: "Asia/Shanghai",
    });
    expect(push).toHaveBeenCalledWith("/scheduler");
  });
});
