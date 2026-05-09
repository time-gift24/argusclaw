import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createRouter, createMemoryHistory } from "vue-router";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));
vi.mock("@opentiny/tiny-robot", async () => import("@/test/stubs/tiny-robot"));

import AdminLayout from "./AdminLayout.vue";
import HealthPage from "@/features/health/HealthPage.vue";
import BootstrapPage from "@/features/bootstrap/BootstrapPage.vue";
import ProvidersPage from "@/features/providers/ProvidersPage.vue";
import TemplatesPage from "@/features/templates/TemplatesPage.vue";
import McpPage from "@/features/mcp/McpPage.vue";
import ToolsPage from "@/features/tools/ToolsPage.vue";
import SchedulerPage from "@/features/scheduler/SchedulerPage.vue";
import AgentRunsPage from "@/features/agent-runs/AgentRunsPage.vue";
import ChatPage from "@/features/chat/ChatPage.vue";
import RuntimePage from "@/features/runtime/RuntimePage.vue";

describe("AdminLayout", () => {
  it("renders a left-nav management shell", async () => {
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        {
          path: "/",
          component: AdminLayout,
          children: [
            { path: "", component: BootstrapPage },
            { path: "health", component: HealthPage },
            { path: "runtime", component: RuntimePage },
            { path: "providers", component: ProvidersPage },
            { path: "templates", component: TemplatesPage },
            { path: "mcp", component: McpPage },
            { path: "tools", component: ToolsPage },
            { path: "scheduler", component: SchedulerPage },
            { path: "agent-runs", component: AgentRunsPage },
            { path: "chat", component: ChatPage },
          ],
        },
      ],
    });
    await router.push("/");
    await router.isReady();

    const wrapper = mount(AdminLayout, {
      global: {
        plugins: [router],
        stubs: {
          RouterView: { template: "<div />" },
        },
      },
    });

    expect(wrapper.find(".sidebar").exists()).toBe(true);
    expect(wrapper.find(".route-shell").exists()).toBe(true);
    expect(wrapper.text()).toContain("ArgusWing");
    expect(wrapper.text()).not.toContain("ArgusClaw");
    expect(wrapper.text()).toContain("单实例");
    expect(wrapper.text()).toContain("概览");
    expect(wrapper.text()).toContain("健康检查");
    expect(wrapper.text()).toContain("运行状态");
    expect(wrapper.text()).toContain("模型提供方");
    expect(wrapper.text()).toContain("智能体模板");
    expect(wrapper.text()).toContain("工具注册表");
    expect(wrapper.text()).toContain("Scheduler");
    expect(wrapper.text()).toContain("Agent Runs");
    expect(wrapper.text()).toContain("对话");
    expect(wrapper.find(".topbar").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("Desktop Server Web Admin");
  });
});
