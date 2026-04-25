import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import AgentRunsPage from "./AgentRunsPage.vue";
import {
  resetApiClient,
  setApiClient,
  type AgentRecord,
  type AgentRunDetail,
  type AgentRunSummary,
  type ApiClient,
} from "@/lib/api";

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
          captured_at: "2026-04-25T00:00:00Z",
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
          captured_at: "2026-04-25T00:00:00Z",
        },
        runtimes: [],
      },
    }),
    listProviders: async () => [],
    saveProvider: async (input) => input,
    listTemplates: async () => [templateRecord()],
    saveTemplate: async (input) => input,
    listMcpServers: async () => [],
    saveMcpServer: async (input) => input,
    ...overrides,
  } as ApiClient;
}

describe("AgentRunsPage", () => {
  afterEach(() => {
    resetApiClient();
  });

  it("creates an agent run and refreshes its status", async () => {
    const createdRun: AgentRunSummary = {
      run_id: "run-1",
      agent_id: 7,
      status: "queued",
      created_at: "2026-04-25T00:00:00Z",
      updated_at: "2026-04-25T00:00:00Z",
    };
    const completedRun: AgentRunDetail = {
      ...createdRun,
      status: "completed",
      prompt: "Inspect the plan",
      result: "Done",
      error: null,
      updated_at: "2026-04-25T00:00:01Z",
      completed_at: "2026-04-25T00:00:01Z",
    };
    const createAgentRun = vi.fn(async () => createdRun);
    const getAgentRun = vi.fn(async () => completedRun);
    setApiClient(makeApiClient({
      createAgentRun,
      getAgentRun,
    }));

    const wrapper = mount(AgentRunsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("Agent Runs");
    expect(wrapper.text()).toContain("Planner");

    await wrapper.get('[data-testid="agent-run-prompt"]').setValue("Inspect the plan");
    await wrapper.get('[data-testid="create-agent-run"]').trigger("click");
    await flushPromises();

    expect(createAgentRun).toHaveBeenCalledWith({
      agent_id: 7,
      prompt: "Inspect the plan",
    });
    expect(wrapper.text()).toContain("run-1");
    expect(wrapper.text()).toContain("queued");

    await wrapper.get('[data-testid="refresh-agent-run"]').trigger("click");
    await flushPromises();

    expect(getAgentRun).toHaveBeenCalledWith("run-1");
    expect(wrapper.text()).toContain("completed");
    expect(wrapper.text()).toContain("Done");
  });
});
