import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import ToolsPage from "./ToolsPage.vue";
import { resetApiClient, setApiClient, type ApiClient, type LlmProviderRecord, type ToolRegistryItem } from "@/lib/api";

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

describe("ToolsPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("shows tool registry summaries and risk groups", async () => {
    const tools: ToolRegistryItem[] = [
      {
        name: "shell",
        description: "Execute shell commands",
        risk_level: "critical",
        parameters: { type: "object", properties: { command: { type: "string" } } },
      },
      {
        name: "read",
        description: "Read files",
        risk_level: "high",
        parameters: { type: "object" },
      },
      {
        name: "scheduler",
        description: "Schedule agent work",
        risk_level: "medium",
        parameters: { oneOf: [] },
      },
    ];
    const mockApi: ApiClient = {
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
      getRuntimeState: async () => emptyRuntimeState(),
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input) => input as LlmProviderRecord,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
      listTools: async () => tools,
    };
    setApiClient(mockApi);

    const wrapper = mount(ToolsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("工具注册表");
    expect(wrapper.text()).toContain("总工具");
    expect(wrapper.text()).toContain("高风险及以上");
    expect(wrapper.text()).toContain("shell");
    expect(wrapper.text()).toContain("critical");
    expect(wrapper.text()).toContain("Execute shell commands");
    expect(wrapper.text()).toContain("command");
  });
});
