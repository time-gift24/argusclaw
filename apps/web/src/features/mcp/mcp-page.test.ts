import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import McpPage from "./McpPage.vue";
import { resetApiClient, setApiClient, type ApiClient } from "@/lib/api";

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
    };
    setApiClient(mockApi);

    const wrapper = mount(McpPage);

    await flushPromises();
    expect(wrapper.text()).toContain("Docs MCP");
  });
});
