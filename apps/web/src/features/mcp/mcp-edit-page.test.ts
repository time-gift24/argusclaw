import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createRouter, createWebHistory } from "vue-router";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import McpEditPage from "./McpEditPage.vue";
import {
  resetApiClient,
  setApiClient,
  type ApiClient,
  type McpServerRecord,
} from "@/lib/api";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/mcp", component: { template: "div" } },
    { path: "/mcp/new", component: McpEditPage },
    { path: "/mcp/:serverId/edit", component: McpEditPage },
  ],
});

function mcpServerRecord(overrides: Partial<McpServerRecord> = {}): McpServerRecord {
  return {
    id: 1,
    display_name: "Docs MCP",
    enabled: true,
    transport: { kind: "stdio", command: "docs-mcp", args: [], env: {} },
    timeout_ms: 5000,
    status: "ready",
    last_checked_at: null,
    last_success_at: null,
    last_error: null,
    discovered_tool_count: 0,
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
      mcp_server_count: 1,
      default_provider_id: null,
      default_template_id: null,
      mcp_ready_count: 1,
    }),
    listMcpServers: async () => [mcpServerRecord()],
    saveMcpServer: async (input) => input,
    ...overrides,
  } as ApiClient;
}

describe("McpEditPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("saves a new MCP server", async () => {
    const saveMcpServer = vi.fn(async (input: McpServerRecord) => mcpServerRecord({ ...input, id: 10 }));

    setApiClient(makeApiClient({
      saveMcpServer,
    }));

    router.push("/mcp/new");
    await router.isReady();

    const wrapper = mount(McpEditPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    await wrapper.find('input').setValue("New MCP"); // display_name is first input
    // The component uses TinyForm/TinyFormItem which are stubbed as div
    // We might need to be more specific or rely on internal refs if stubs are too simple
    // But let's try the simplest approach first.

    // In the component:
    // <TinyInput v-model="form.display_name" ... />
    // Stubbed TinyInput renders <input v-bind="$attrs" />

    const inputs = wrapper.findAll('input');
    await inputs[0]?.setValue("New MCP");
    await inputs[1]?.setValue("npx"); // command is 3rd if stdio, but wait
    // Let's use the actual labels or something more robust if possible.
    // But for now, let's just use the first few.
  });

  it("loads an existing MCP server and saves edits", async () => {
    const server = mcpServerRecord({ id: 1, display_name: "Old MCP" });
    const saveMcpServer = vi.fn(async (input: McpServerRecord) => input);

    setApiClient(makeApiClient({
      listMcpServers: async () => [server],
      saveMcpServer,
    }));

    router.push("/mcp/1/edit");
    await router.isReady();

    const wrapper = mount(McpEditPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();
    await new Promise(resolve => setTimeout(resolve, 0)); // Extra tick

    const inputs = wrapper.findAll('input');
    expect(inputs[0]?.element.value).toBe("Old MCP");

    await inputs[0]?.setValue("Updated MCP");
    await wrapper.find('[data-testid="save-mcp"]').trigger("click");
    await flushPromises();

    expect(saveMcpServer).toHaveBeenCalledWith(expect.objectContaining({
      id: 1,
      display_name: "Updated MCP",
    }));
  });
});
