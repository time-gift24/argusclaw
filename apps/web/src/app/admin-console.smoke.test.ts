import { describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import App from "@/App.vue";
import router from "@/router";
import { setApiClient, resetApiClient, type ApiClient } from "@/lib/api";

describe("admin console", () => {
  it("exposes core management entry points", async () => {
    const mockApi: ApiClient = {
      getHealth: async () => ({ status: "ok" }),
      getBootstrap: async () => ({
        instance_name: "Workspace Admin",
        provider_count: 1,
        template_count: 1,
        mcp_server_count: 1,
        default_provider_id: 1,
        default_template_id: 2,
        mcp_ready_count: 1,
      }),
      getSettings: async () => ({
        instance_name: "Workspace Admin",
        default_provider_id: 1,
        default_provider_name: "OpenAI",
      }),
      updateSettings: async () => ({
        instance_name: "Workspace Admin",
        default_provider_id: 1,
        default_provider_name: "OpenAI",
      }),
      listProviders: async () => [],
      saveProvider: async (input) => input,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
    };
    setApiClient(mockApi);

    await router.push("/");
    await router.isReady();

    const wrapper = mount(App, {
      global: {
        plugins: [router],
      },
    });

    await flushPromises();
    expect(wrapper.text()).toContain("概览");
    expect(wrapper.text()).toContain("健康检查");
    expect(wrapper.text()).toContain("模型提供方");
    expect(wrapper.text()).toContain("智能体模板");
    expect(wrapper.text()).toContain("MCP 服务");
    expect(wrapper.text()).toContain("系统设置");

    resetApiClient();
  });
});
