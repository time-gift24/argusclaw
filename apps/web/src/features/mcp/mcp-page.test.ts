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

  it("creates a stdio MCP server and tests the draft connection", async () => {
    const createdServer: McpServerRecord = {
      id: 9,
      display_name: "Docs MCP",
      enabled: true,
      transport: { kind: "stdio", command: "docs-mcp", args: ["--stdio", "--verbose"], env: { DOCS_TOKEN: "abc" } },
      timeout_ms: 7000,
      status: "connecting",
      last_checked_at: null,
      last_success_at: null,
      last_error: null,
      discovered_tool_count: 0,
    };
    const listMcpServers = vi
      .fn<() => Promise<McpServerRecord[]>>()
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([createdServer]);
    const saveMcpServer = vi.fn<(input: McpServerRecord) => Promise<McpServerRecord>>().mockResolvedValue(createdServer);
    const testMcpServerDraft = vi.fn<(input: McpServerRecord) => Promise<McpConnectionTestResult>>().mockResolvedValue({
      status: "failed",
      checked_at: "2026-04-23T12:00:00Z",
      latency_ms: 8,
      discovered_tools: [],
      message: "missing binary",
    });

    setApiClient({
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
      saveProvider: async (input) => input,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers,
      saveMcpServer,
      deleteMcpServer: async () => ({ deleted: true }),
      testMcpServer: async () => ({
        status: "ready",
        checked_at: "2026-04-23T12:00:00Z",
        latency_ms: 10,
        discovered_tools: [],
        message: "connection succeeded",
      }),
      testMcpServerDraft,
      listMcpServerTools: async () => [],
    });

    const wrapper = mount(McpPage);
    await flushPromises();

    await wrapper.get('[name="mcp-display-name"]').setValue("Docs MCP");
    await wrapper.get('[name="mcp-command"]').setValue("docs-mcp");
    await wrapper.get('[name="mcp-args"]').setValue("--stdio\n--verbose");
    await wrapper.get('[name="mcp-env"]').setValue("DOCS_TOKEN=abc");
    await wrapper.get('[name="mcp-timeout"]').setValue("7000");

    await wrapper.get('[data-testid="test-mcp-draft"]').trigger("click");
    await flushPromises();
    expect(testMcpServerDraft).toHaveBeenCalledWith(
      expect.objectContaining({
        display_name: "Docs MCP",
        enabled: true,
        timeout_ms: 7000,
        transport: { kind: "stdio", command: "docs-mcp", args: ["--stdio", "--verbose"], env: { DOCS_TOKEN: "abc" } },
      }),
    );
    expect(wrapper.text()).toContain("missing binary");

    await wrapper.get('[data-testid="mcp-form"]').trigger("submit");
    await flushPromises();
    expect(saveMcpServer).toHaveBeenCalledWith(
      expect.objectContaining({
        id: null,
        display_name: "Docs MCP",
        transport: { kind: "stdio", command: "docs-mcp", args: ["--stdio", "--verbose"], env: { DOCS_TOKEN: "abc" } },
      }),
    );
    expect(wrapper.text()).toContain("MCP 服务已创建。");
    expect(wrapper.text()).toContain("Docs MCP");
  });

  it("edits an existing HTTP MCP server", async () => {
    const server: McpServerRecord = {
      id: 12,
      display_name: "Remote MCP",
      enabled: true,
      transport: { kind: "http", url: "https://old.example.com/mcp", headers: { Authorization: "Bearer old" } },
      timeout_ms: 5000,
      status: "ready",
      last_checked_at: null,
      last_success_at: null,
      last_error: null,
      discovered_tool_count: 2,
    };
    const updatedServer: McpServerRecord = {
      ...server,
      display_name: "Remote MCP Updated",
      transport: { kind: "http", url: "https://new.example.com/mcp", headers: { Authorization: "Bearer new" } },
      timeout_ms: 9000,
    };
    const listMcpServers = vi
      .fn<() => Promise<McpServerRecord[]>>()
      .mockResolvedValueOnce([server])
      .mockResolvedValueOnce([updatedServer]);
    const saveMcpServer = vi.fn<(input: McpServerRecord) => Promise<McpServerRecord>>().mockResolvedValue(updatedServer);

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
      saveMcpServer,
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

    await wrapper.get('[data-testid="edit-mcp-12"]').trigger("click");
    await flushPromises();
    expect((wrapper.get('[name="mcp-display-name"]').element as HTMLInputElement).value).toBe("Remote MCP");

    await wrapper.get('[name="mcp-display-name"]').setValue("Remote MCP Updated");
    await wrapper.get('[name="mcp-url"]').setValue("https://new.example.com/mcp");
    await wrapper.get('[name="mcp-headers"]').setValue("Authorization=Bearer new");
    await wrapper.get('[name="mcp-timeout"]').setValue("9000");
    await wrapper.get('[data-testid="mcp-form"]').trigger("submit");
    await flushPromises();

    expect(saveMcpServer).toHaveBeenCalledWith(
      expect.objectContaining({
        id: 12,
        display_name: "Remote MCP Updated",
        timeout_ms: 9000,
        transport: { kind: "http", url: "https://new.example.com/mcp", headers: { Authorization: "Bearer new" } },
      }),
    );
    expect(wrapper.text()).toContain("MCP 服务已更新。");
  });
});
