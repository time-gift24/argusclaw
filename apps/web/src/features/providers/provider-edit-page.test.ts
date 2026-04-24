import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRouter, createWebHistory } from "vue-router";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import ProviderEditPage from "./ProviderEditPage.vue";
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

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/providers", component: { template: "div" } },
    { path: "/providers/new", component: ProviderEditPage },
    { path: "/providers/:providerId/edit", component: ProviderEditPage },
  ],
});

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
    getSettings: vi.fn<() => Promise<SettingsResponse>>().mockResolvedValue({
      instance_name: "Argus",
      default_provider_id: 1,
      default_provider_name: "OpenAI",
    }),
    listProviders: vi.fn<() => Promise<LlmProviderRecord[]>>().mockResolvedValue([]),
    saveProvider: vi.fn<(input: LlmProviderRecord) => Promise<LlmProviderRecord>>().mockImplementation(
      async (input) => input,
    ),
    testProviderDraft: vi.fn<(input: LlmProviderRecord) => Promise<ProviderTestResult>>().mockResolvedValue({
      provider_id: "0",
      model: "gpt-4.1",
      base_url: "https://api.openai.com/v1",
      checked_at: "2026-04-23T12:00:00Z",
      latency_ms: 10,
      status: "success",
      message: "连接成功",
    }),
    ...overrides,
  } as ApiClient;
}

afterEach(() => {
  resetApiClient();
});

describe("ProviderEditPage", () => {
  it("saves a new provider", async () => {
    const saveProvider = vi
      .fn<(input: LlmProviderRecord) => Promise<LlmProviderRecord>>()
      .mockImplementation(async (input) =>
        makeProvider({ ...input, id: 2 }),
      );

    setApiClient(
      makeApiClient({
        saveProvider,
      }),
    );

    router.push("/providers/new");
    await router.isReady();

    const wrapper = mount(ProviderEditPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

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
    });
  });

  it("loads an existing provider and saves edits", async () => {
    const provider = makeProvider({ id: 1, display_name: "OpenAI" });
    const listProviders = vi.fn<() => Promise<LlmProviderRecord[]>>().mockResolvedValue([provider]);
    const saveProvider = vi.fn().mockImplementation(async (input) => makeProvider(input));

    setApiClient(
      makeApiClient({
        listProviders,
        saveProvider,
      }),
    );

    router.push("/providers/1/edit");
    await router.isReady();

    const wrapper = mount(ProviderEditPage, {
      global: {
        plugins: [router],
        mocks: {
          $route: {
            params: { providerId: "1" }
          }
        }
      },
    });
    await flushPromises();
    await new Promise(resolve => setTimeout(resolve, 0)); // Extra tick

    expect((wrapper.get('input[name="display-name"]').element as HTMLInputElement).value).toBe("OpenAI");

    await wrapper.get('input[name="display-name"]').setValue("OpenAI Mirror");
    await wrapper.get("form").trigger("submit");
    await flushPromises();

    expect(saveProvider).toHaveBeenCalledTimes(1);
    expect(saveProvider.mock.calls[0]?.[0]).toMatchObject({
      id: 1,
      display_name: "OpenAI Mirror",
      api_key: null,
    });
  });
});
