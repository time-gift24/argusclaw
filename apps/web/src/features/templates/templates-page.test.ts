import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import TemplatesPage from "./TemplatesPage.vue";
import {
  resetApiClient,
  setApiClient,
  type AgentRecord,
  type ApiClient,
  type LlmProviderRecord,
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
    getRuntimeState: async () => emptyRuntimeState(),
    getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
    updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
    listProviders: async () => [],
    saveProvider: async (input) => input,
    listTemplates: async () => [templateRecord()],
    saveTemplate: async (input) => input,
    deleteTemplate: async () => ({ deleted: true }),
    listMcpServers: async () => [],
    saveMcpServer: async (input) => input,
    ...overrides,
  };
}

describe("TemplatesPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("shows template inventory", async () => {
    setApiClient(makeApiClient());

    const wrapper = mount(TemplatesPage);

    await flushPromises();
    expect(wrapper.text()).toContain("Planner");
  });

  it("deletes a template and refreshes the inventory", async () => {
    const listTemplates = vi
      .fn()
      .mockResolvedValueOnce([templateRecord({ id: 8, display_name: "Disposable Planner" })])
      .mockResolvedValueOnce([]);
    const deleteTemplate = vi.fn(async () => ({ deleted: true }));

    setApiClient(makeApiClient({
      listTemplates,
      deleteTemplate,
    }));

    const wrapper = mount(TemplatesPage);
    await flushPromises();

    await wrapper.get('[data-testid="delete-template-8"]').trigger("click");
    await flushPromises();

    expect(deleteTemplate).toHaveBeenCalledWith(8);
    expect(wrapper.text()).toContain("暂无可用的模板");
  });

  it("creates a template from the form and refreshes the inventory", async () => {
    const createdTemplate = templateRecord({
      id: 12,
      display_name: "代码助手",
      description: "协助代码实现和审查。",
      provider_id: 7,
      model_id: "glm-4.7",
      system_prompt: "You are a careful coding agent.",
      tool_names: ["read", "shell"],
      subagent_names: ["reviewer"],
      max_tokens: 4096,
      temperature: 0.2,
      thinking_config: { type: "enabled", clear_thinking: true },
    });
    const listTemplates = vi.fn().mockResolvedValueOnce([]).mockResolvedValueOnce([createdTemplate]);
    const saveTemplate = vi.fn(async () => createdTemplate);

    setApiClient(
      makeApiClient({
        listProviders: async () => [providerRecord()],
        listTemplates,
        saveTemplate,
      }),
    );

    const wrapper = mount(TemplatesPage);
    await flushPromises();

    await wrapper.get('[data-testid="template-display-name"]').setValue("代码助手");
    await wrapper.get('[data-testid="template-description"]').setValue("协助代码实现和审查。");
    await wrapper.get('[data-testid="template-provider"]').setValue("7");
    await wrapper.get('[data-testid="template-model"]').setValue("glm-4.7");
    await wrapper.get('[data-testid="template-system-prompt"]').setValue("You are a careful coding agent.");
    await wrapper.get('[data-testid="template-tools"]').setValue("read\nshell");
    await wrapper.get('[data-testid="template-subagents"]').setValue("reviewer");
    await wrapper.get('[data-testid="template-max-tokens"]').setValue("4096");
    await wrapper.get('[data-testid="template-temperature"]').setValue("0.2");
    await wrapper.get('[data-testid="template-thinking"]').setValue(true);
    await wrapper.get('[data-testid="create-template"]').trigger("click");
    await flushPromises();

    expect(saveTemplate).toHaveBeenCalledWith({
      id: 0,
      display_name: "代码助手",
      description: "协助代码实现和审查。",
      version: "1.0.0",
      provider_id: 7,
      model_id: "glm-4.7",
      system_prompt: "You are a careful coding agent.",
      tool_names: ["read", "shell"],
      subagent_names: ["reviewer"],
      max_tokens: 4096,
      temperature: 0.2,
      thinking_config: { type: "enabled", clear_thinking: true },
    });
    expect(wrapper.text()).toContain("模板已创建。");
    expect(wrapper.text()).toContain("代码助手");
  });

  it("does not submit an incomplete template", async () => {
    const saveTemplate = vi.fn(async (input: AgentRecord) => input);
    setApiClient(makeApiClient({ saveTemplate }));

    const wrapper = mount(TemplatesPage);
    await flushPromises();

    await wrapper.get('[data-testid="template-display-name"]').setValue(" ");
    await wrapper.get('[data-testid="create-template"]').trigger("click");
    await flushPromises();

    expect(saveTemplate).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("请填写模板名称和系统提示词。");
  });
});
