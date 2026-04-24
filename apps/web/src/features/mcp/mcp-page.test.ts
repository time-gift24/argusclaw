import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createRouter, createWebHistory } from "vue-router";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import McpPage from "./McpPage.vue";
import {
  resetApiClient,
  setApiClient,
  type ApiClient,
  type LlmProviderRecord,
  type McpDiscoveredToolRecord,
  type McpServerRecord,
} from "@/lib/api";

const router = createRouter({
  history: createWebHistory(),
  routes: [{ path: "/mcp", component: McpPage }],
});

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

function makeApiClient(overrides: Partial<ApiClient> = {}): ApiClient {
  return {
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
    saveProvider: async (input) => input as LlmProviderRecord,
    listTemplates: async () => [],
    saveTemplate: async (input) => input,
    listMcpServers: async () => [],
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
    ...overrides,
  } as ApiClient;
}

describe("McpPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("shows configured MCP servers", async () => {
    setApiClient(makeApiClient({
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
    }));

    const wrapper = mount(McpPage, {
      global: {
        plugins: [router],
      },
    });

    await flushPromises();
    expect(wrapper.text()).toContain("Docs MCP");
    expect(wrapper.text()).toContain("总服务");
    expect(wrapper.text()).toContain("就绪服务");
    expect(wrapper.text()).toContain("已发现工具");
  });

  it("shows operational diagnostics for servers that need attention", async () => {
    setApiClient(makeApiClient({
      listMcpServers: async () => [
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
    }));

    const wrapper = mount(McpPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

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
    const testMcpServer = vi.fn<(serverId: number) => Promise<any>>().mockResolvedValue({
      status: "failed",
      checked_at: "2026-04-23T12:00:00Z",
      latency_ms: 10,
      discovered_tools: [],
      message: "missing binary",
    });
    const deleteMcpServer = vi.fn<(serverId: number) => Promise<{ deleted: boolean }>>().mockResolvedValue({
      deleted: true,
    });

    setApiClient(makeApiClient({
      listMcpServers,
      listMcpServerTools,
      testMcpServer,
      deleteMcpServer,
    }));

    const wrapper = mount(McpPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    await wrapper.find('.server-actions button').trigger("click"); // 查看工具
    await flushPromises();
    expect(listMcpServerTools).toHaveBeenCalledWith(4);
    expect(wrapper.text()).toContain("search_docs");

    const buttons = wrapper.findAll('.server-actions button');
    await buttons[1]?.trigger("click"); // 测试连接
    await flushPromises();
    expect(testMcpServer).toHaveBeenCalledWith(4);
    expect(wrapper.text()).toContain("missing binary");

    await buttons[3]?.trigger("click"); // 删除
    await flushPromises();
    expect(deleteMcpServer).toHaveBeenCalledWith(4);
    expect(wrapper.text()).toContain("暂无已配置的 MCP 服务");
  });
});
