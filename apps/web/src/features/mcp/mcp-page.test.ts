import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import McpPage from "./McpPage.vue";
import {
  resetApiClient,
  setApiClient,
  type ApiClient,
  type McpConnectionTestResult,
  type McpDiscoveredToolRecord,
  type McpServerRecord,
} from "@/lib/api";

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

describe("McpPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("shows configured MCP servers", async () => {
    const mockApi: ApiClient = {
      getHealth: async () => ({ status: "ok" }),
      getBootstrap: async () => ({
        instance_name: "",
        provider_count: 0,
        template_count: 0,
        mcp_server_count: 1,
        default_provider_id: null,
        default_template_id: null,
        mcp_ready_count: 1,
      }),
      getRuntimeState: async () => emptyRuntimeState(),
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input) => input,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers: async () => [
        {
          id: 2,
          display_name: "Docs MCP",
          enabled: true,
          transport: { kind: "stdio", command: "docs-mcp", args: ["--stdio"], env: {} },
          timeout_ms: 5000,
          status: "ready",
          last_checked_at: null,
          last_success_at: null,
          last_error: null,
          discovered_tool_count: 3,
        },
      ],
      saveMcpServer: async (input) => input,
      deleteMcpServer: async () => ({ deleted: true }),
      testMcpServer: async () => ({
        status: "ready",
        checked_at: "2026-04-23T12:00:00Z",
        latency_ms: 10,
        discovered_tools: [],
        message: "connection succeeded",
      }),
      listMcpServerTools: async () => [],
    };
    setApiClient(mockApi);

    const wrapper = mount(McpPage);

    await flushPromises();
    expect(wrapper.text()).toContain("Docs MCP");
    expect(wrapper.text()).toContain("总服务");
    expect(wrapper.text()).toContain("就绪服务");
    expect(wrapper.text()).toContain("已发现工具");
  });

  it("shows operational diagnostics for servers that need attention", async () => {
    setApiClient({
      getHealth: async () => ({ status: "ok" }),
      getBootstrap: async () => ({
        instance_name: "",
        provider_count: 0,
        template_count: 0,
        mcp_server_count: 2,
        default_provider_id: null,
        default_template_id: null,
        mcp_ready_count: 1,
      }),
      getRuntimeState: async () => emptyRuntimeState(),
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input) => input,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers: async () => [
        {
          id: 2,
          display_name: "Docs MCP",
          enabled: true,
          transport: { kind: "stdio", command: "docs-mcp", args: ["--stdio"], env: {} },
          timeout_ms: 5000,
          status: "ready",
          last_checked_at: "2026-04-23T12:00:00Z",
          last_success_at: "2026-04-23T12:00:01Z",
          last_error: null,
          discovered_tool_count: 3,
        },
        {
          id: 3,
          display_name: "Broken MCP",
          enabled: true,
          transport: { kind: "stdio", command: "missing-mcp", args: [], env: {} },
          timeout_ms: 3000,
          status: "failed",
          last_checked_at: "2026-04-23T12:05:00Z",
          last_success_at: null,
          last_error: "spawn failed",
          discovered_tool_count: 0,
        },
      ],
      saveMcpServer: async (input) => input,
      deleteMcpServer: async () => ({ deleted: true }),
      testMcpServer: async () => ({
        status: "ready",
        checked_at: "2026-04-23T12:00:00Z",
        latency_ms: 10,
        discovered_tools: [],
        message: "connection succeeded",
      }),
      listMcpServerTools: async () => [],
    });

    const wrapper = mount(McpPage);
    await flushPromises();

    expect(wrapper.text()).toContain("需关注");
    expect(wrapper.text()).toContain("Broken MCP");
    expect(wrapper.text()).toContain("stdio：missing-mcp");
    expect(wrapper.text()).toContain("最近错误：spawn failed");
  });

  it("loads tools, tests, and deletes an MCP server", async () => {
    const server: McpServerRecord = {
      id: 4,
      display_name: "Disposable MCP",
      enabled: true,
      transport: { kind: "stdio", command: "docs-mcp", args: ["--stdio"], env: {} },
      timeout_ms: 5000,
      status: "ready",
      last_checked_at: null,
      last_success_at: null,
      last_error: null,
      discovered_tool_count: 1,
    };
    const listMcpServers = vi.fn<() => Promise<McpServerRecord[]>>().mockResolvedValueOnce([server]).mockResolvedValueOnce([]);
    const listMcpServerTools = vi.fn<(serverId: number) => Promise<McpDiscoveredToolRecord[]>>().mockResolvedValue([
      {
        server_id: 4,
        tool_name_original: "search_docs",
        description: "Search docs",
        schema: { type: "object" },
        annotations: null,
      },
    ]);
    const testMcpServer = vi.fn<(serverId: number) => Promise<McpConnectionTestResult>>().mockResolvedValue({
      status: "failed",
      checked_at: "2026-04-23T12:00:00Z",
      latency_ms: 10,
      discovered_tools: [],
      message: "missing binary",
    });
    const deleteMcpServer = vi.fn<(serverId: number) => Promise<{ deleted: boolean }>>().mockResolvedValue({
      deleted: true,
    });

    setApiClient({
      getHealth: async () => ({ status: "ok" }),
      getBootstrap: async () => ({
        instance_name: "",
        provider_count: 0,
        template_count: 0,
        mcp_server_count: 1,
        default_provider_id: null,
        default_template_id: null,
        mcp_ready_count: 1,
      }),
      getRuntimeState: async () => emptyRuntimeState(),
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input) => input,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers,
      saveMcpServer: async (input) => input,
      deleteMcpServer,
      testMcpServer,
      listMcpServerTools,
    });

    const wrapper = mount(McpPage);
    await flushPromises();

    await wrapper.get('[data-testid="tools-mcp-4"]').trigger("click");
    await flushPromises();
    expect(listMcpServerTools).toHaveBeenCalledWith(4);
    expect(wrapper.text()).toContain("search_docs");
    expect(wrapper.text()).toContain("Search docs");
    expect(wrapper.text()).toContain("Schema");
    expect(wrapper.text()).toContain("object");

    await wrapper.get('[data-testid="test-mcp-4"]').trigger("click");
    await flushPromises();
    expect(testMcpServer).toHaveBeenCalledWith(4);
    expect(wrapper.text()).toContain("missing binary");

    await wrapper.get('[data-testid="delete-mcp-4"]').trigger("click");
    await flushPromises();
    expect(deleteMcpServer).toHaveBeenCalledWith(4);
    expect(wrapper.text()).toContain("暂无已配置的 MCP 服务");
  });
});
