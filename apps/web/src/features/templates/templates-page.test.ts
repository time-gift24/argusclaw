import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import TemplatesPage from "./TemplatesPage.vue";
import { resetApiClient, setApiClient, type ApiClient } from "@/lib/api";

function emptyRuntimeState() {
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
        captured_at: "2026-04-23T12:00:00Z",
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
        captured_at: "2026-04-23T12:00:00Z",
      },
      runtimes: [],
    },
  };
}

describe("TemplatesPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("shows template inventory", async () => {
    const mockApi: ApiClient = {
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
      getRuntimeState: async () => emptyRuntimeState(),
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input) => input,
      listTemplates: async () => [
        {
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
        },
      ],
      saveTemplate: async (input) => input,
      deleteTemplate: async () => ({ deleted: true }),
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
    };
    setApiClient(mockApi);

    const wrapper = mount(TemplatesPage);

    await flushPromises();
    expect(wrapper.text()).toContain("Planner");
  });

  it("deletes a template and refreshes the inventory", async () => {
    const listTemplates = vi
      .fn()
      .mockResolvedValueOnce([
        {
          id: 8,
          display_name: "Disposable Planner",
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
        },
      ])
      .mockResolvedValueOnce([]);
    const deleteTemplate = vi.fn(async () => ({ deleted: true }));

    setApiClient({
      getHealth: async () => ({ status: "ok" }),
      getBootstrap: async () => ({
        instance_name: "",
        provider_count: 0,
        template_count: 1,
        mcp_server_count: 0,
        default_provider_id: null,
        default_template_id: 8,
        mcp_ready_count: 0,
      }),
      getRuntimeState: async () => emptyRuntimeState(),
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input) => input,
      listTemplates,
      saveTemplate: async (input) => input,
      deleteTemplate,
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
    });

    const wrapper = mount(TemplatesPage);
    await flushPromises();

    await wrapper.get('[data-testid="delete-template-8"]').trigger("click");
    await flushPromises();

    expect(deleteTemplate).toHaveBeenCalledWith(8);
    expect(wrapper.text()).toContain("暂无可用的模板");
  });
});
