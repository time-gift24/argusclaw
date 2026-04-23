import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import HealthPage from "./HealthPage.vue";
import { resetApiClient, setApiClient, type ApiClient } from "@/lib/api";

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
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input) => input,
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
});
