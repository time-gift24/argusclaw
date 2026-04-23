import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import RuntimePage from "./RuntimePage.vue";
import { resetApiClient, setApiClient, type AgentRecord, type ApiClient, type LlmProviderRecord, type McpServerRecord } from "@/lib/api";

describe("RuntimePage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("shows runtime snapshot summaries", async () => {
    const mockApi: ApiClient = {
      getHealth: async () => ({ status: "ok" }),
      getBootstrap: async () => ({
        instance_name: "ArgusWing",
        provider_count: 2,
        template_count: 3,
        mcp_server_count: 1,
        default_provider_id: 1,
        default_template_id: 2,
        mcp_ready_count: 1,
      }),
      getRuntimeState: async () => ({
        thread_pool: {
          snapshot: {
            max_threads: 8,
            active_threads: 2,
            queued_threads: 1,
            running_threads: 1,
            cooling_threads: 0,
            evicted_threads: 0,
            estimated_memory_bytes: 4096,
            peak_estimated_memory_bytes: 8192,
            process_memory_bytes: 16384,
            peak_process_memory_bytes: 32768,
            resident_thread_count: 2,
            avg_thread_memory_bytes: 2048,
            captured_at: "2026-04-23T12:00:00Z",
          },
          runtimes: [
            {
              thread_id: "thread-1",
              session_id: "session-1",
              status: "running",
              estimated_memory_bytes: 2048,
              last_active_at: "2026-04-23T12:00:00Z",
              recoverable: true,
              last_reason: null,
            },
          ],
        },
        job_runtime: {
          snapshot: {
            max_threads: 8,
            active_threads: 1,
            queued_threads: 0,
            running_threads: 1,
            cooling_threads: 0,
            evicted_threads: 0,
            estimated_memory_bytes: 1024,
            peak_estimated_memory_bytes: 2048,
            process_memory_bytes: 16384,
            peak_process_memory_bytes: 32768,
            resident_thread_count: 1,
            avg_thread_memory_bytes: 1024,
            captured_at: "2026-04-23T12:00:00Z",
          },
          runtimes: [
            {
              thread_id: "thread-job-1",
              job_id: "job-1",
              status: "queued",
              estimated_memory_bytes: 1024,
              last_active_at: "2026-04-23T12:00:00Z",
              recoverable: true,
              last_reason: "memory_pressure",
            },
          ],
        },
      }),
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input: LlmProviderRecord) => input,
      listTemplates: async () => [],
      saveTemplate: async (input: AgentRecord) => input,
      listMcpServers: async () => [],
      saveMcpServer: async (input: McpServerRecord) => input,
    };
    setApiClient(mockApi);

    const wrapper = mount(RuntimePage);

    await flushPromises();
    expect(wrapper.text()).toContain("运行时总览");
    expect(wrapper.text()).toContain("线程池活跃数");
    expect(wrapper.text()).toContain("thread-1");
    expect(wrapper.text()).toContain("job-1");
    expect(wrapper.text()).toContain("内存压力");
  });

  it("updates runtime summaries from the event stream", async () => {
    const close = vi.fn();
    const mockApi = {
      getHealth: async () => ({ status: "ok" }),
      getBootstrap: async () => ({
        instance_name: "ArgusWing",
        provider_count: 2,
        template_count: 3,
        mcp_server_count: 1,
        default_provider_id: 1,
        default_template_id: 2,
        mcp_ready_count: 1,
      }),
      getRuntimeState: async () => ({
        thread_pool: {
          snapshot: {
            max_threads: 8,
            active_threads: 1,
            queued_threads: 0,
            running_threads: 0,
            cooling_threads: 0,
            evicted_threads: 0,
            estimated_memory_bytes: 1024,
            peak_estimated_memory_bytes: 1024,
            process_memory_bytes: null,
            peak_process_memory_bytes: null,
            resident_thread_count: 1,
            avg_thread_memory_bytes: 1024,
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
      subscribeRuntimeState: vi.fn((handlers) => {
        handlers.onSnapshot({
          thread_pool: {
            snapshot: {
              max_threads: 8,
              active_threads: 3,
              queued_threads: 2,
              running_threads: 1,
              cooling_threads: 0,
              evicted_threads: 0,
              estimated_memory_bytes: 3072,
              peak_estimated_memory_bytes: 4096,
              process_memory_bytes: null,
              peak_process_memory_bytes: null,
              resident_thread_count: 3,
              avg_thread_memory_bytes: 1024,
              captured_at: "2026-04-23T12:00:05Z",
            },
            runtimes: [],
          },
          job_runtime: {
            snapshot: {
              max_threads: 8,
              active_threads: 1,
              queued_threads: 0,
              running_threads: 1,
              cooling_threads: 0,
              evicted_threads: 0,
              estimated_memory_bytes: 1024,
              peak_estimated_memory_bytes: 1024,
              process_memory_bytes: null,
              peak_process_memory_bytes: null,
              resident_thread_count: 1,
              avg_thread_memory_bytes: 1024,
              captured_at: "2026-04-23T12:00:05Z",
            },
            runtimes: [],
          },
        });
        return { close };
      }),
      getSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      updateSettings: async () => ({ instance_name: "", default_provider_id: null, default_provider_name: null }),
      listProviders: async () => [],
      saveProvider: async (input: LlmProviderRecord) => input,
      listTemplates: async () => [],
      saveTemplate: async (input: AgentRecord) => input,
      listMcpServers: async () => [],
      saveMcpServer: async (input: McpServerRecord) => input,
    } as unknown as ApiClient;
    setApiClient(mockApi);

    const wrapper = mount(RuntimePage);

    await flushPromises();
    expect(wrapper.text()).toContain("事件流已连接");
    expect(wrapper.text()).toContain("线程池活跃数3");
    wrapper.unmount();
    expect(close).toHaveBeenCalledOnce();
  });
});
