import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import BootstrapPage from "./BootstrapPage.vue";
import { resetApiClient, setApiClient, type ApiClient } from "@/lib/api";

describe("BootstrapPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("shows instance bootstrap data", async () => {
    const mockApi: ApiClient = {
      getHealth: async () => ({ status: "ok" }),
      getBootstrap: async () => ({
        instance_name: "Workspace Admin",
        provider_count: 2,
        template_count: 3,
        mcp_server_count: 1,
        default_provider_id: 12,
        default_template_id: null,
        mcp_ready_count: 1,
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
      }),
      getSettings: async () => ({
        instance_name: "",
        default_provider_id: null,
        default_provider_name: null,
      }),
      updateSettings: async () => ({
        instance_name: "",
        default_provider_id: null,
        default_provider_name: null,
      }),
      listProviders: async () => [],
      saveProvider: async (input) => input,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
    };
    setApiClient(mockApi);

    const wrapper = mount(BootstrapPage);

    await flushPromises();
    expect(wrapper.text()).toContain("Workspace Admin");
    expect(wrapper.text()).toContain("12");
    expect(wrapper.text()).toContain("MCP 服务");
  });
});
