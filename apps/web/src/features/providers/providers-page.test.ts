import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRouter, createWebHistory } from "vue-router";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import ProvidersPage from "./ProvidersPage.vue";
import {
  resetApiClient,
  setApiClient,
  type AccountStatus,
  type LlmProviderRecord,
  type ApiClient,
} from "@/lib/api";

const router = createRouter({
  history: createWebHistory(),
  routes: [{ path: "/providers", component: ProvidersPage }],
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

function makeApiClient(overrides: Partial<ApiClient> = {}): ApiClient {
  return {
    getHealth: vi.fn().mockResolvedValue({ status: "ok" }),
    getBootstrap: vi.fn().mockResolvedValue({
      instance_name: "Argus",
      provider_count: 1,
      template_count: 0,
      mcp_server_count: 0,
      default_provider_id: 1,
      default_template_id: null,
      mcp_ready_count: 0,
    }),
    getAccountStatus: vi.fn<() => Promise<AccountStatus>>().mockResolvedValue({
      configured: false,
      username: null,
    }),
    configureAccount: vi
      .fn<(input: { username: string; password: string }) => Promise<AccountStatus>>()
      .mockImplementation(async (input) => ({
        configured: true,
        username: input.username,
      })),
    listProviders: vi.fn().mockResolvedValue([]),
    testProvider: vi.fn().mockResolvedValue({
      status: "success",
      message: "连接成功",
    }),
    deleteProvider: vi.fn().mockResolvedValue({ deleted: true }),
    ...overrides,
  } as ApiClient;
}

afterEach(() => {
  resetApiClient();
});

describe("ProvidersPage", () => {
  it("loads provider rows from the server", async () => {
    const listProviders = vi.fn().mockResolvedValue([makeProvider()]);

    setApiClient(makeApiClient({ listProviders }));

    const wrapper = mount(ProvidersPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    expect(wrapper.text()).toContain("OpenAI");
  });

  it("tests and deletes a provider with visible feedback", async () => {
    const listProviders = vi
      .fn()
      .mockResolvedValueOnce([makeProvider({ id: 3, display_name: "Disposable Provider" })])
      .mockResolvedValueOnce([]);

    setApiClient(makeApiClient({
      listProviders,
      testProvider: vi.fn().mockResolvedValue({
        status: "success",
        message: "connection refused",
      }),
    }));

    const wrapper = mount(ProvidersPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    await wrapper.get('[data-testid="test-provider-3"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("connection refused");

    await wrapper.get('[data-testid="delete-provider-3"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("暂无已配置的提供方");
  });

  it("shows load errors instead of leaving the page silent", async () => {
    setApiClient(makeApiClient({
      listProviders: vi.fn().mockRejectedValue(new Error("server offline")),
    }));

    const wrapper = mount(ProvidersPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    expect(wrapper.text()).toContain("server offline");
  });

  it("shows and saves server account credentials from the providers page", async () => {
    const configureAccount = vi
      .fn<(input: { username: string; password: string }) => Promise<AccountStatus>>()
      .mockResolvedValue({
        configured: true,
        username: "alice",
      });

    setApiClient(makeApiClient({ configureAccount }));

    const wrapper = mount(ProvidersPage, {
      global: {
        plugins: [router],
      },
    });
    await flushPromises();

    expect(wrapper.text()).toContain("服务端账号凭据");

    await wrapper.get('input[name="account-username"]').setValue("alice");
    await wrapper.get('input[name="account-password"]').setValue("first-secret");
    await wrapper.get('[data-testid="save-account"]').trigger("click");
    await flushPromises();

    expect(configureAccount).toHaveBeenCalledWith({
      username: "alice",
      password: "first-secret",
    });
    expect(wrapper.text()).toContain("账号凭据已保存：alice");
  });
});
