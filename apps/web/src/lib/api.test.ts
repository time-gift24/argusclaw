import { afterEach, describe, expect, it, vi } from "vitest";

import { resetApiClient, getApiClient } from "./api";

describe("HttpApiClient", () => {
  afterEach(() => {
    resetApiClient();
    vi.unstubAllGlobals();
  });

  it("surfaces structured server error messages", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: false,
      status: 502,
      headers: {
        get: () => "application/json",
      },
      json: async () => ({
        error: {
          code: "bad_gateway",
          message: "上游服务不可用，请稍后重试。",
        },
      }),
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(getApiClient().getHealth()).rejects.toThrow(
      "上游服务不可用，请稍后重试。",
    );
  });

  it("creates and fetches agent runs with the server data envelope", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({
        ok: true,
        headers: {
          get: () => "application/json",
        },
        json: async () => ({
          data: {
            run_id: "run-1",
            agent_id: 7,
            status: "queued",
            created_at: "2026-04-25T00:00:00Z",
            updated_at: "2026-04-25T00:00:00Z",
          },
        }),
      })
      .mockResolvedValueOnce({
        ok: true,
        headers: {
          get: () => "application/json",
        },
        json: async () => ({
          run_id: "run-1",
          agent_id: 7,
          status: "completed",
          prompt: "Inspect the plan",
          result: "Done",
          error: null,
          created_at: "2026-04-25T00:00:00Z",
          updated_at: "2026-04-25T00:00:01Z",
          completed_at: "2026-04-25T00:00:01Z",
        }),
      });
    vi.stubGlobal("fetch", fetchMock);

    const client = getApiClient();
    expect(client.createAgentRun).toBeDefined();
    expect(client.getAgentRun).toBeDefined();

    const created = await client.createAgentRun!({
      agent_id: 7,
      prompt: "Inspect the plan",
      mcp_headers: {
        tenant: {
          Authorization: "Bearer runtime",
        },
      },
    });
    const detail = await client.getAgentRun!("run-1");

    expect(fetchMock).toHaveBeenNthCalledWith(1, "/api/v1/agents/runs", {
      body: JSON.stringify({
        agent_id: 7,
        prompt: "Inspect the plan",
        mcp_headers: {
          tenant: {
            Authorization: "Bearer runtime",
          },
        },
      }),
      headers: {
        "Content-Type": "application/json",
      },
      method: "POST",
    });
    expect(fetchMock).toHaveBeenNthCalledWith(2, "/api/v1/agents/runs/run-1", undefined);
    expect(created.run_id).toBe("run-1");
    expect(created.status).toBe("queued");
    expect(detail.status).toBe("completed");
    expect(detail.result).toBe("Done");
  });
});
