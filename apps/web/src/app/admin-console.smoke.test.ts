import { describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));
vi.mock("@opentiny/tiny-robot", async () => import("@/test/stubs/tiny-robot"));

import App from "@/App.vue";
import router from "@/router";
import { setApiClient, resetApiClient, type ApiClient, type LlmProviderRecord } from "@/lib/api";

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
      getRuntimeState: async () => ({
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
      }),
      listProviders: async () => [],
      saveProvider: async (input) => input as LlmProviderRecord,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
      listChatSessions: async () => [],
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
    expect(wrapper.text()).toContain("运行状态");
    expect(wrapper.text()).toContain("模型提供方");
    expect(wrapper.text()).toContain("智能体模板");
    expect(wrapper.text()).toContain("MCP 服务");
    expect(wrapper.text()).toContain("工具注册表");
    expect(wrapper.text()).toContain("Agent Runs");
    expect(wrapper.text()).toContain("对话");
    expect(wrapper.text()).not.toContain("系统设置");

    resetApiClient();
  });
});
