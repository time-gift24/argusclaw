import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import SettingsPage from "./SettingsPage.vue";
import { resetApiClient, setApiClient, type ApiClient } from "@/lib/api";

describe("SettingsPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("loads and saves instance settings", async () => {
    const updateSettings = vi.fn(async () => ({
      instance_name: "Updated Workspace",
      default_provider_id: 5,
      default_provider_name: "Pinned Provider",
    }));

    const mockApi: ApiClient = {
      getHealth: async () => ({ status: "ok" }),
      getBootstrap: async () => ({
        instance_name: "",
        provider_count: 0,
        template_count: 0,
        mcp_server_count: 0,
        default_provider_id: 12,
        default_template_id: null,
        mcp_ready_count: 0,
      }),
      getSettings: async () => ({
        instance_name: "Workspace Admin",
        default_provider_id: 12,
        default_provider_name: "Primary Provider",
      }),
      updateSettings,
      listProviders: async () => [],
      saveProvider: async (input) => input,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
    };
    setApiClient(mockApi);

    const wrapper = mount(SettingsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("Workspace Admin");
    expect(wrapper.text()).toContain("Primary Provider");

    await wrapper.get('input[name="instance-name"]').setValue("Updated Workspace");
    await wrapper.get('input[name="default-provider-id"]').setValue("5");
    await wrapper.get("button").trigger("click");
    await flushPromises();

    expect(updateSettings).toHaveBeenCalledWith({
      instance_name: "Updated Workspace",
      default_provider_id: 5,
    });
    expect(wrapper.text()).toContain("Pinned Provider");
  });
});
