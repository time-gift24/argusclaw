import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createRouter, createWebHistory } from "vue-router";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import TemplateEditPage from "./TemplateEditPage.vue";
import {
  resetApiClient,
  setApiClient,
  type AgentRecord,
  type ApiClient,
  type LlmProviderRecord,
  type McpServerRecord,
  type ToolRegistryItem,
} from "@/lib/api";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/templates", component: { template: "div" } },
    { path: "/templates/new", component: TemplateEditPage },
    { path: "/templates/:templateId/edit", component: TemplateEditPage },
  ],
});

function templateRecord(overrides: Partial<AgentRecord> = {}): AgentRecord {
  return {
    id: 7,
    display_name: "Planner",
    description: "Plans safely",
    version: "1.0.0",
    provider_id: null,
    model_id: null,
    system_prompt: "",
    tool_names: [],
    mcp_bindings: [],
    subagent_names: [],
    max_tokens: null,
    temperature: null,
    thinking_config: null,
    ...overrides,
  };
}

function providerRecord(overrides: Partial<LlmProviderRecord> = {}): LlmProviderRecord {
  return {
    id: 7,
    kind: "openai-compatible",
    display_name: "Z.AI",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
    api_key: "",
    models: ["glm-4.7", "glm-4-plus"],
    model_config: {},
    default_model: "glm-4.7",
    is_default: true,
    extra_headers: {},
    secret_status: "ready",
    meta_data: {},
    ...overrides,
  };
}

function toolRecord(overrides: Partial<ToolRegistryItem> = {}): ToolRegistryItem {
  return {
    name: "shell",
    description: "Execute shell commands",
    risk_level: "critical",
    parameters: {},
    ...overrides,
  };
}

function mcpServerRecord(overrides: Partial<McpServerRecord> = {}): McpServerRecord {
  return {
    id: 4,
    display_name: "Slack MCP",
    enabled: true,
    transport: {
      kind: "stdio",
      command: "slack-mcp",
      args: [],
      env: {},
    },
    timeout_ms: 5000,
    status: "ready",
    last_checked_at: null,
    last_success_at: null,
    last_error: null,
    discovered_tool_count: 2,
    ...overrides,
  };
}

function makeApiClient(overrides: Partial<ApiClient> = {}): ApiClient {
  return {
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
    listProviders: async () => [providerRecord()],
    listTemplates: async () => [templateRecord()],
    saveTemplate: async (input) => input,
    listTools: async () => [],
    listMcpServers: async () => [],
    ...overrides,
  } as ApiClient;
}

describe("TemplateEditPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("creates a template from the form", async () => {
    const saveTemplate = vi.fn(async (input: AgentRecord) => templateRecord({ ...input, id: 12 }));

    setApiClient(makeApiClient({
      saveTemplate,
    }));

    router.push("/templates/new");
    await router.isReady();

    const wrapper = mount(TemplateEditPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    await wrapper.get('[data-testid="template-display-name"]').setValue("代码助手");
    await wrapper.get('[data-testid="template-system-prompt"]').setValue("You are a coding agent.");
    await wrapper.get('[data-testid="save-template"]').trigger("click");
    await flushPromises();

    expect(saveTemplate).toHaveBeenCalledWith(expect.objectContaining({
      display_name: "代码助手",
      system_prompt: "You are a coding agent.",
      tool_names: [],
      subagent_names: [],
    }));
  });

  it("places the system prompt after the template options with a larger editor", async () => {
    setApiClient(makeApiClient());

    router.push("/templates/new");
    await router.isReady();

    const wrapper = mount(TemplateEditPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    const html = wrapper.html();
    expect(html.indexOf('data-testid="template-system-prompt"')).toBeGreaterThan(
      html.indexOf('data-testid="template-thinking"'),
    );
    expect(wrapper.get('[data-testid="template-system-prompt"]').attributes("rows")).toBe("12");
  });

  it("renders a safe markdown preview below the system prompt editor", async () => {
    setApiClient(makeApiClient());

    router.push("/templates/new");
    await router.isReady();

    const wrapper = mount(TemplateEditPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    await wrapper.get('[data-testid="template-system-prompt"]').setValue([
      "# 角色",
      "- 保持简洁",
      "```",
      "cargo test",
      "```",
      "<script>alert('xss')</script>",
    ].join("\n"));

    const preview = wrapper.get('[data-testid="template-system-prompt-preview"]');
    expect(preview.find("h1").text()).toBe("角色");
    expect(preview.find("li").text()).toBe("保持简洁");
    expect(preview.find("pre code").text()).toBe("cargo test");
    expect(preview.find("script").exists()).toBe(false);
    expect(preview.text()).toContain("<script>alert('xss')</script>");
  });

  it("loads an existing template and saves edits", async () => {
    const template = templateRecord({ id: 7, display_name: "Planner", system_prompt: "Plan well" });
    const saveTemplate = vi.fn(async (input: AgentRecord) => input);

    setApiClient(makeApiClient({
      listTemplates: async () => [template],
      saveTemplate,
    }));

    router.push("/templates/7/edit");
    await router.isReady();

    const wrapper = mount(TemplateEditPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();
    await new Promise(resolve => setTimeout(resolve, 0)); // Extra tick

    expect((wrapper.get('[data-testid="template-display-name"]').element as HTMLInputElement).value).toBe("Planner");

    await wrapper.get('[data-testid="template-display-name"]').setValue("Expert Planner");
    await wrapper.get('[data-testid="save-template"]').trigger("click");
    await flushPromises();

    expect(saveTemplate).toHaveBeenCalledWith(expect.objectContaining({
      id: 7,
      display_name: "Expert Planner",
    }));
  });

  it("shows tool, mcp, and subagent descriptions while preserving selected values", async () => {
    const saveTemplate = vi.fn(async (input: AgentRecord) => templateRecord({ ...input, id: 12 }));

    setApiClient(makeApiClient({
      listTemplates: async () => [
        templateRecord({ id: 7, display_name: "Planner", description: "Plans safely" }),
        templateRecord({ id: 8, display_name: "Reviewer", description: "Reviews implementation risk" }),
      ],
      listTools: async () => [toolRecord()],
      listMcpServers: async () => [mcpServerRecord()],
      saveTemplate,
    }));

    router.push("/templates/new");
    await router.isReady();

    const wrapper = mount(TemplateEditPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    expect(wrapper.find('[content="Execute shell commands"]').exists()).toBe(true);
    expect(wrapper.find('[content="状态：就绪；已发现工具 2 个"]').exists()).toBe(true);
    expect(wrapper.find('[content="Reviews implementation risk"]').exists()).toBe(true);

    await wrapper.get('[data-testid="template-display-name"]').setValue("带工具模板");
    await wrapper.get('[data-testid="template-system-prompt"]').setValue("Use tools carefully.");
    await wrapper.get('[data-testid="template-tools"]').setValue(["shell"]);
    await wrapper.get('[data-testid="template-mcp-servers"]').setValue(["4"]);
    await wrapper.get('[data-testid="template-subagents"]').setValue(["Reviewer"]);
    await wrapper.get('[data-testid="save-template"]').trigger("click");
    await flushPromises();

    expect(saveTemplate).toHaveBeenCalledWith(expect.objectContaining({
      tool_names: ["shell"],
      mcp_bindings: [{ server_id: 4, allowed_tools: null }],
      subagent_names: ["Reviewer"],
    }));
  });
});
