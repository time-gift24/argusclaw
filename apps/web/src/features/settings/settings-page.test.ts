import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import SettingsPage from "./SettingsPage.vue";
import { resetApiClient, setApiClient, type ApiClient, type LlmProviderRecord } from "@/lib/api";

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
      getSettings: async () => ({
        instance_name: "Workspace Admin",
        default_provider_id: 12,
        default_provider_name: "Primary Provider",
      }),
      updateSettings,
      listProviders: async () => [],
      saveProvider: async (input) => input as LlmProviderRecord,
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

  it("shows save errors", async () => {
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
      getSettings: async () => ({
        instance_name: "Workspace Admin",
        default_provider_id: 12,
        default_provider_name: "Primary Provider",
      }),
      updateSettings: async () => {
        throw new Error("settings write failed");
      },
      listProviders: async () => [],
      saveProvider: async (input) => input as LlmProviderRecord,
      listTemplates: async () => [],
      saveTemplate: async (input) => input,
      listMcpServers: async () => [],
      saveMcpServer: async (input) => input,
    };
    setApiClient(mockApi);

    const wrapper = mount(SettingsPage);
    await flushPromises();

    await wrapper.get("button").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("settings write failed");
  });
});
