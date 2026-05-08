import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createRouter, createWebHistory } from "vue-router";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import TemplatesPage from "./TemplatesPage.vue";
import {
  resetApiClient,
  setApiClient,
  type AgentRecord,
  type ApiClient,
} from "@/lib/api";

const router = createRouter({
  history: createWebHistory(),
  routes: [{ path: "/templates", component: TemplatesPage }],
});

function templateRecord(overrides: Partial<AgentRecord> = {}): AgentRecord {
  return {
    id: 7,
    display_name: "Planner",
    description: "Plans safely",
    version: "1.0.0",
    provider_id: null,
    model_id: null,
    system_prompt: "",
    tool_names: [],
    subagent_names: [],
    max_tokens: null,
    temperature: null,
    thinking_config: null,
    ...overrides,
  };
}

function makeApiClient(overrides: Partial<ApiClient> = {}): ApiClient {
  return {
    getHealth: async () => ({ status: "ok" }),
    getBootstrap: async () => ({
      instance_name: "",
      provider_count: 0,
      template_count: 1,
      mcp_server_count: 0,
      default_provider_id: null,
      default_template_id: 7,
      mcp_ready_count: 0,
    }),
    listProviders: async () => [],
    listTemplates: async () => [templateRecord()],
    deleteTemplate: async () => ({
      deleted: true,
      agent_deleted: true,
      deleted_job_count: 0,
      deleted_run_count: 0,
      deleted_thread_count: 0,
      deleted_session_count: 0,
    }),
    ...overrides,
  } as ApiClient;
}

describe("TemplatesPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("shows template inventory", async () => {
    setApiClient(makeApiClient());

    const wrapper = mount(TemplatesPage, {
      global: {
        plugins: [router],
      },
    });

    await flushPromises();
    expect(wrapper.text()).toContain("Planner");
  });

  it("deletes a template and refreshes the inventory", async () => {
    const listTemplates = vi
      .fn()
      .mockResolvedValueOnce([templateRecord({ id: 8, display_name: "Disposable Planner" })])
      .mockResolvedValueOnce([]);
    const deleteTemplate = vi.fn(async () => ({
      deleted: true,
      agent_deleted: true,
      deleted_job_count: 0,
      deleted_run_count: 0,
      deleted_thread_count: 0,
      deleted_session_count: 0,
    }));

    setApiClient(
      makeApiClient({
        listTemplates,
        deleteTemplate,
      }),
    );

    const wrapper = mount(TemplatesPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    await wrapper.get('[data-testid="delete-template-8"]').trigger("click");
    await flushPromises();

    expect(deleteTemplate).toHaveBeenCalledWith(8);
    expect(wrapper.text()).toContain("暂无可用的模板");
  });

  it("confirms cascade deletion when references block direct delete", async () => {
    const listTemplates = vi
      .fn()
      .mockResolvedValueOnce([templateRecord({ id: 8, display_name: "Busy Planner" })])
      .mockResolvedValueOnce([]);
    const deleteTemplate = vi
      .fn()
      .mockRejectedValueOnce(
        new Error("无法删除智能体 8：当前仍被 1 个会话线程引用，请先移除相关会话、任务或调度配置。"),
      )
      .mockResolvedValueOnce({
        deleted: true,
        agent_deleted: true,
        deleted_job_count: 1,
        deleted_run_count: 1,
        deleted_thread_count: 2,
        deleted_session_count: 1,
      });

    setApiClient(
      makeApiClient({
        listTemplates,
        deleteTemplate,
      }),
    );

    const wrapper = mount(TemplatesPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    await wrapper.get('[data-testid="delete-template-8"]').trigger("click");
    await flushPromises();

    expect(wrapper.find('[data-testid="cascade-delete-confirmation"]').exists()).toBe(true);

    await wrapper.get('[data-testid="confirm-cascade-delete"]').trigger("click");
    await flushPromises();

    expect(deleteTemplate).toHaveBeenNthCalledWith(1, 8);
    expect(deleteTemplate).toHaveBeenNthCalledWith(2, 8, { cascadeAssociations: true });
    expect(wrapper.text()).toContain(
      "模板及关联的 1 个任务、1 条运行记录、2 个线程、1 个空会话 已删除。",
    );
  });
});
