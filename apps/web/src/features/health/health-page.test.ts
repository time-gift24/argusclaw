import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import HealthPage from "./HealthPage.vue";
import { resetApiClient, setApiClient, type ApiClient, type LlmProviderRecord } from "@/lib/api";

describe("HealthPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("shows service status", async () => {
    const mockApi: ApiClient = {
      getHealth: async () => ({ status: "ok" }),
      getBootstrap: async () => ({
        instance_name: "Workspace Admin",
        provider_count: 2,
        template_count: 3,
        mcp_server_count: 1,
        default_provider_id: 1,
        default_template_id: 2,
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
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input) => input as LlmProviderRecord,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
    };
    setApiClient(mockApi);

    const wrapper = mount(HealthPage);

    await flushPromises();
    expect(wrapper.text()).toContain("健康");
    expect(wrapper.text()).toContain("Workspace Admin");
  });

  it("shows an unhealthy error state when the server cannot be reached", async () => {
    const mockApi: ApiClient = {
      getHealth: async () => {
        throw new Error("Request failed: 502");
      },
      getBootstrap: async () => ({
        instance_name: "Workspace Admin",
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
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input) => input as LlmProviderRecord,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
    };
    setApiClient(mockApi);

    const wrapper = mount(HealthPage);

    await flushPromises();
    expect(wrapper.text()).toContain("异常");
    expect(wrapper.text()).toContain("Request failed: 502");
  });
});
