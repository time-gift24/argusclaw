import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import TemplatesPage from "./TemplatesPage.vue";
import { resetApiClient, setApiClient, type ApiClient } from "@/lib/api";

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
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
    };
    setApiClient(mockApi);

    const wrapper = mount(TemplatesPage);

    await flushPromises();
    expect(wrapper.text()).toContain("Planner");
  });
});
