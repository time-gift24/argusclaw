import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import ProvidersPage from "./ProvidersPage.vue";
import {
  resetApiClient,
  setApiClient,
  type AgentRecord,
  type ApiClient,
  type BootstrapResponse,
  type HealthResponse,
  type LlmProviderRecord,
  type McpServerRecord,
  type ProviderTestResult,
  type SettingsResponse,
  type UpdateSettingsRequest,
} from "@/lib/api";

function makeProvider(overrides: Partial<LlmProviderRecord> = {}): LlmProviderRecord {
  return {
    id: 1,
    kind: "openai-compatible",
    display_name: "OpenAI",
    base_url: "https://api.openai.com/v1",
    api_key: "",
    models: ["gpt-4.1"],
    model_config: {},
    default_model: "gpt-4.1",
    is_default: true,
    extra_headers: {},
    secret_status: "ready",
    meta_data: {},
    ...overrides,
  };
}

function makeApiClient(overrides: Partial<ApiClient>): ApiClient {
  return {
    getHealth: vi.fn<() => Promise<HealthResponse>>().mockResolvedValue({ status: "ok" }),
    getBootstrap: vi.fn<() => Promise<BootstrapResponse>>().mockResolvedValue({
      instance_name: "Argus",
      provider_count: 1,
      template_count: 0,
      mcp_server_count: 0,
      default_provider_id: 1,
      default_template_id: null,
      mcp_ready_count: 0,
    }),
    getRuntimeState: vi.fn<() => Promise<import("@/lib/api").RuntimeStateResponse>>().mockResolvedValue({
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
    getSettings: vi.fn<() => Promise<SettingsResponse>>().mockResolvedValue({
      instance_name: "Argus",
      default_provider_id: 1,
      default_provider_name: "OpenAI",
    }),
    updateSettings: vi
      .fn<(input: UpdateSettingsRequest) => Promise<SettingsResponse>>()
      .mockImplementation(async (input) => ({
        instance_name: input.instance_name,
        default_provider_id: input.default_provider_id,
        default_provider_name: input.default_provider_id ? "OpenAI" : null,
      })),
    listProviders: vi.fn<() => Promise<LlmProviderRecord[]>>().mockResolvedValue([]),
    saveProvider: vi.fn<(input: LlmProviderRecord) => Promise<LlmProviderRecord>>().mockImplementation(
      async (input) => input,
    ),
    deleteProvider: vi.fn<(providerId: number) => Promise<{ deleted: boolean }>>().mockResolvedValue({
      deleted: true,
    }),
    testProvider: vi.fn<(providerId: number, model?: string) => Promise<ProviderTestResult>>().mockResolvedValue({
      provider_id: "1",
      model: "gpt-4.1",
      base_url: "https://api.openai.com/v1",
      checked_at: "2026-04-23T12:00:00Z",
      latency_ms: 10,
      status: "success",
      message: "连接成功",
    }),
    testProviderDraft: vi.fn<(input: LlmProviderRecord) => Promise<ProviderTestResult>>().mockResolvedValue({
      provider_id: "0",
      model: "gpt-4.1",
      base_url: "https://api.openai.com/v1",
      checked_at: "2026-04-23T12:00:00Z",
      latency_ms: 10,
      status: "success",
      message: "连接成功",
    }),
    listTemplates: vi.fn<() => Promise<AgentRecord[]>>().mockResolvedValue([]),
    saveTemplate: vi.fn<(input: AgentRecord) => Promise<AgentRecord>>().mockImplementation(
      async (input) => input,
    ),
    listMcpServers: vi.fn<() => Promise<McpServerRecord[]>>().mockResolvedValue([]),
    saveMcpServer: vi
      .fn<(input: McpServerRecord) => Promise<McpServerRecord>>()
      .mockImplementation(async (input) => input),
    ...overrides,
  };
}

afterEach(() => {
  resetApiClient();
});

describe("ProvidersPage", () => {
  it("loads provider rows from the server and saves a new provider", async () => {
    const listProviders = vi
      .fn<() => Promise<LlmProviderRecord[]>>()
      .mockResolvedValueOnce([makeProvider()])
      .mockResolvedValueOnce([
        makeProvider(),
        makeProvider({
          id: 2,
          display_name: "Azure Mirror",
          base_url: "https://azure.example.com/openai/v1",
          default_model: "gpt-4.1-mini",
          is_default: false,
        }),
      ]);

    const saveProvider = vi
      .fn<(input: LlmProviderRecord) => Promise<LlmProviderRecord>>()
      .mockImplementation(async (input) =>
        makeProvider({
          ...input,
          id: 2,
          display_name: "Azure Mirror",
          base_url: "https://azure.example.com/openai/v1",
          default_model: "gpt-4.1-mini",
          is_default: false,
        }),
      );

    setApiClient(
      makeApiClient({
        listProviders,
        saveProvider,
      }),
    );

    const wrapper = mount(ProvidersPage);
    await flushPromises();

    const panels = wrapper.findAll("article");
    expect(panels[0]?.classes()).toContain("list-panel");
    expect(panels[1]?.classes()).toContain("form-panel");

    expect(wrapper.text()).toContain("OpenAI");

    await wrapper.get('input[name="display-name"]').setValue("Azure Mirror");
    await wrapper.get('input[name="base-url"]').setValue("https://azure.example.com/openai/v1");
    await wrapper.get('input[name="api-key"]').setValue("secret-token");
    await wrapper.get('input[name="models"]').setValue("gpt-4.1-mini");
    await wrapper.get('input[name="default-model"]').setValue("gpt-4.1-mini");
    await wrapper.get("form").trigger("submit");
    await flushPromises();

    expect(saveProvider).toHaveBeenCalledTimes(1);
    expect(saveProvider.mock.calls[0]?.[0]).toMatchObject({
      display_name: "Azure Mirror",
      base_url: "https://azure.example.com/openai/v1",
      default_model: "gpt-4.1-mini",
      models: ["gpt-4.1-mini"],
    });
    expect(listProviders).toHaveBeenCalledTimes(2);
    expect(wrapper.text()).toContain("Azure Mirror");
  });

  it("loads an existing provider into the form and saves edits", async () => {
    const listProviders = vi
      .fn<() => Promise<LlmProviderRecord[]>>()
      .mockResolvedValueOnce([makeProvider()])
      .mockResolvedValueOnce([
        makeProvider({
          display_name: "OpenAI Mirror",
          base_url: "https://mirror.example.com/v1",
        }),
      ]);

    const saveProvider = vi
      .fn<(input: LlmProviderRecord) => Promise<LlmProviderRecord>>()
      .mockImplementation(async (input) =>
        makeProvider({
          ...input,
          display_name: "OpenAI Mirror",
          base_url: "https://mirror.example.com/v1",
        }),
      );

    setApiClient(
      makeApiClient({
        listProviders,
        saveProvider,
      }),
    );

    const wrapper = mount(ProvidersPage);
    await flushPromises();

    await wrapper.get('[data-testid="edit-provider-1"]').trigger("click");
    await wrapper.get('input[name="display-name"]').setValue("OpenAI Mirror");
    await wrapper.get('input[name="base-url"]').setValue("https://mirror.example.com/v1");
    await wrapper.get("form").trigger("submit");
    await flushPromises();

    expect(saveProvider).toHaveBeenCalledTimes(1);
    expect(saveProvider.mock.calls[0]?.[0]).toMatchObject({
      id: 1,
      display_name: "OpenAI Mirror",
      base_url: "https://mirror.example.com/v1",
    });
    expect(wrapper.text()).toContain("OpenAI Mirror");
  });

  it("tests and deletes a provider with visible feedback", async () => {
    const listProviders = vi
      .fn<() => Promise<LlmProviderRecord[]>>()
      .mockResolvedValueOnce([makeProvider({ id: 3, display_name: "Disposable Provider", is_default: false })])
      .mockResolvedValueOnce([]);
    const testProvider = vi.fn<(providerId: number, model?: string) => Promise<ProviderTestResult>>().mockResolvedValue({
      provider_id: "3",
      model: "gpt-4.1",
      base_url: "https://api.openai.com/v1",
      checked_at: "2026-04-23T12:00:00Z",
      latency_ms: 5,
      status: "request_failed",
      message: "connection refused",
    });
    const deleteProvider = vi.fn<(providerId: number) => Promise<{ deleted: boolean }>>().mockResolvedValue({
      deleted: true,
    });

    setApiClient(
      makeApiClient({
        listProviders,
        testProvider,
        deleteProvider,
      }),
    );

    const wrapper = mount(ProvidersPage);
    await flushPromises();

    await wrapper.get('[data-testid="test-provider-3"]').trigger("click");
    await flushPromises();
    expect(testProvider).toHaveBeenCalledWith(3, "gpt-4.1");
    expect(wrapper.text()).toContain("connection refused");

    await wrapper.get('[data-testid="delete-provider-3"]').trigger("click");
    await flushPromises();
    expect(deleteProvider).toHaveBeenCalledWith(3);
    expect(wrapper.text()).toContain("暂无已配置的提供方");
  });

  it("shows load errors instead of leaving the page silent", async () => {
    setApiClient(
      makeApiClient({
        listProviders: vi.fn<() => Promise<LlmProviderRecord[]>>().mockRejectedValue(new Error("server offline")),
      }),
    );

    const wrapper = mount(ProvidersPage);
    await flushPromises();

    expect(wrapper.text()).toContain("server offline");
  });
});
