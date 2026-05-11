import { flushPromises, mount } from "@vue/test-utils";
import { createMemoryHistory, createRouter } from "vue-router";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));
vi.mock("@opentiny/tiny-robot", async () => import("@/test/stubs/tiny-robot"));

import JobChatPage from "./JobChatPage.vue";
import appRouter from "@/router";
import {
  resetApiClient,
  setApiClient,
  type ApiClient,
  type ChatJobConversation,
  type ChatMessageRecord,
} from "@/lib/api";

function message(
  role: ChatMessageRecord["role"],
  content: string,
  overrides: Partial<ChatMessageRecord> = {},
): ChatMessageRecord {
  return {
    role,
    content,
    reasoning_content: null,
    content_parts: [],
    tool_call_id: null,
    name: null,
    tool_calls: null,
    metadata: null,
    ...overrides,
  };
}

function conversation(overrides: Partial<ChatJobConversation> = {}): ChatJobConversation {
  return {
    job_id: "job-1",
    title: "job:job-1",
    status: "succeeded",
    thread_id: "thread-job",
    session_id: "thread-job",
    parent_session_id: "session-1",
    parent_thread_id: "thread-1",
    messages: [
      message("user", "执行子任务"),
      message("assistant", "子任务完成"),
    ],
    turn_count: 1,
    token_count: 42,
    plan_item_count: 0,
    ...overrides,
  };
}

function makeApiClient(item: ChatJobConversation): ApiClient {
  return {
    getChatJobConversation: vi.fn().mockResolvedValue(item),
  } as unknown as ApiClient;
}

async function mountJobChatPage(
  item: ChatJobConversation | ApiClient,
  initialPath = "/chat/jobs/job-1",
) {
  const apiClient = "getChatJobConversation" in item ? item : makeApiClient(item);
  setApiClient(apiClient);

  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: "/chat", name: "chat", component: { template: "<div />" } },
      { path: "/chat/jobs/:jobId", name: "chat-job", component: JobChatPage },
    ],
  });
  await router.push(initialPath);
  await router.isReady();

  const wrapper = mount(JobChatPage, {
    global: {
      plugins: [router],
    },
  });
  await flushPromises();

  return { apiClient, router, wrapper };
}

function deferredConversation() {
  let resolve!: (value: ChatJobConversation) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<ChatJobConversation>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, reject, resolve };
}

afterEach(() => {
  resetApiClient();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("JobChatPage", () => {
  it("is registered as an immersive sibling chat job route", () => {
    const resolved = appRouter.resolve({ name: "chat-job", params: { jobId: "job-1" } });

    expect(resolved.path).toBe("/chat/jobs/job-1");
    expect(resolved.meta).toMatchObject({
      breadcrumb: "Job 对话",
      immersive: true,
      hideRouteHeader: true,
    });
  });

  it("renders a read-only job conversation with breadcrumb and no composer", async () => {
    const { wrapper } = await mountJobChatPage(conversation());

    expect(wrapper.text()).toContain("对话");
    expect(wrapper.text()).toContain("Job job-1");
    expect(wrapper.text()).toContain("子任务完成");
    expect(wrapper.text()).toContain("返回父对话");
    expect(wrapper.find("[data-testid='chat-input']").exists()).toBe(false);
  });

  it("renders a pending notice when the execution thread is not ready", async () => {
    const { wrapper } = await mountJobChatPage(conversation({
      thread_id: null,
      session_id: null,
      messages: [],
    }));

    expect(wrapper.text()).toContain("执行线程尚未就绪");
    expect(wrapper.text()).not.toContain("快速开始");
    expect(wrapper.find(".prompt-panel").exists()).toBe(false);
  });

  it("returns to the parent conversation from conversation parent ids", async () => {
    const { router, wrapper } = await mountJobChatPage(
      conversation(),
      "/chat/jobs/job-1?fromSession=query-session&fromThread=query-thread",
    );

    await wrapper.get("button").trigger("click");
    await flushPromises();

    expect(router.currentRoute.value).toMatchObject({
      name: "chat",
      query: {
        session: "session-1",
        thread: "thread-1",
      },
    });
  });

  it("returns to query fallback ids when the conversation has no parent ids", async () => {
    const { router, wrapper } = await mountJobChatPage(
      conversation({
        parent_session_id: null,
        parent_thread_id: null,
      }),
      "/chat/jobs/job-1?fromSession=query-session&fromThread=query-thread",
    );

    await wrapper.get("button").trigger("click");
    await flushPromises();

    expect(router.currentRoute.value).toMatchObject({
      name: "chat",
      query: {
        session: "query-session",
        thread: "query-thread",
      },
    });
  });

  it("does not mix partial parent ids with query fallback ids", async () => {
    const { router, wrapper } = await mountJobChatPage(
      conversation({
        parent_session_id: "session-1",
        parent_thread_id: null,
      }),
      "/chat/jobs/job-1?fromThread=query-thread",
    );

    await wrapper.get("button").trigger("click");
    await flushPromises();

    expect(router.currentRoute.value).toMatchObject({
      name: "chat",
      query: {},
    });
  });

  it("uses query fallback ids as a pair when parent ids are incomplete", async () => {
    const { router, wrapper } = await mountJobChatPage(
      conversation({
        parent_session_id: "session-1",
        parent_thread_id: null,
      }),
      "/chat/jobs/job-1?fromSession=query-session&fromThread=query-thread",
    );

    await wrapper.get("button").trigger("click");
    await flushPromises();

    expect(router.currentRoute.value).toMatchObject({
      name: "chat",
      query: {
        session: "query-session",
        thread: "query-thread",
      },
    });
  });

  it("loads the next job when the route param changes", async () => {
    const getChatJobConversation = vi.fn(async (jobId: string) =>
      conversation({
        job_id: jobId,
        title: `job:${jobId}`,
        messages: [
          message("assistant", `loaded ${jobId}`),
        ],
      }));
    const { router, wrapper } = await mountJobChatPage({
      getChatJobConversation,
    } as unknown as ApiClient);

    expect(wrapper.text()).toContain("loaded job-1");

    await router.push("/chat/jobs/job-2");
    await flushPromises();

    expect(getChatJobConversation).toHaveBeenCalledWith("job-1");
    expect(getChatJobConversation).toHaveBeenCalledWith("job-2");
    expect(wrapper.text()).toContain("loaded job-2");
    expect(wrapper.text()).not.toContain("loaded job-1");
  });

  it("ignores stale job conversation responses", async () => {
    const first = deferredConversation();
    const second = deferredConversation();
    const getChatJobConversation = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const { router, wrapper } = await mountJobChatPage({
      getChatJobConversation,
    } as unknown as ApiClient);

    await router.push("/chat/jobs/job-2");
    await flushPromises();

    second.resolve(conversation({
      job_id: "job-2",
      title: "job:job-2",
      messages: [
        message("assistant", "second response"),
      ],
    }));
    await flushPromises();

    first.resolve(conversation({
      job_id: "job-1",
      title: "job:job-1",
      messages: [
        message("assistant", "stale response"),
      ],
    }));
    await flushPromises();

    expect(wrapper.text()).toContain("second response");
    expect(wrapper.text()).not.toContain("stale response");
  });

  it("polls a running job conversation until the assistant response arrives", async () => {
    vi.useFakeTimers();
    const getChatJobConversation = vi
      .fn()
      .mockResolvedValueOnce(conversation({
        status: "running",
        messages: [
          message("user", "执行子任务"),
        ],
      }))
      .mockResolvedValueOnce(conversation({
        status: "succeeded",
        messages: [
          message("user", "执行子任务"),
          message("assistant", "完成"),
        ],
      }));
    const { wrapper } = await mountJobChatPage({
      getChatJobConversation,
    } as unknown as ApiClient);

    expect(getChatJobConversation).toHaveBeenCalledTimes(1);
    expect(wrapper.text()).not.toContain("完成");

    await vi.advanceTimersByTimeAsync(1500);
    await flushPromises();

    expect(getChatJobConversation).toHaveBeenCalledTimes(2);
    expect(wrapper.text()).toContain("完成");
  });

  it("keeps polling the next job when a previous job poll is still unresolved", async () => {
    vi.useFakeTimers();
    const stalePoll = deferredConversation();
    const jobCalls = new Map<string, number>();
    const getChatJobConversation = vi.fn((requestedJobId: string) => {
      const callCount = (jobCalls.get(requestedJobId) ?? 0) + 1;
      jobCalls.set(requestedJobId, callCount);

      if (requestedJobId === "job-1" && callCount === 2) {
        return stalePoll.promise;
      }

      if (requestedJobId === "job-2" && callCount === 2) {
        return Promise.resolve(conversation({
          job_id: "job-2",
          title: "job:job-2",
          status: "succeeded",
          messages: [
            message("assistant", "job-2 refreshed"),
          ],
        }));
      }

      return Promise.resolve(conversation({
        job_id: requestedJobId,
        title: `job:${requestedJobId}`,
        status: "running",
        messages: [
          message("assistant", `${requestedJobId} initial`),
        ],
      }));
    });
    const { router, wrapper } = await mountJobChatPage({
      getChatJobConversation,
    } as unknown as ApiClient);

    await vi.advanceTimersByTimeAsync(1500);
    await flushPromises();
    expect(getChatJobConversation).toHaveBeenCalledTimes(2);

    await router.push("/chat/jobs/job-2");
    await flushPromises();
    expect(wrapper.text()).toContain("job-2 initial");

    await vi.advanceTimersByTimeAsync(1500);
    await flushPromises();

    expect(getChatJobConversation).toHaveBeenCalledTimes(4);
    expect(getChatJobConversation).toHaveBeenLastCalledWith("job-2");
    expect(wrapper.text()).toContain("job-2 refreshed");

    stalePoll.resolve(conversation({
      job_id: "job-1",
      title: "job:job-1",
      status: "succeeded",
      messages: [
        message("assistant", "stale job-1 response"),
      ],
    }));
    await flushPromises();

    expect(wrapper.text()).toContain("job-2 refreshed");
    expect(wrapper.text()).not.toContain("stale job-1 response");
  });

  it("stops polling after a terminal job conversation response", async () => {
    vi.useFakeTimers();
    const getChatJobConversation = vi
      .fn()
      .mockResolvedValueOnce(conversation({ status: "running" }))
      .mockResolvedValueOnce(conversation({
        status: "succeeded",
        messages: [
          message("assistant", "完成"),
        ],
      }))
      .mockResolvedValueOnce(conversation({
        status: "succeeded",
        messages: [
          message("assistant", "不应重复刷新"),
        ],
      }));
    await mountJobChatPage({
      getChatJobConversation,
    } as unknown as ApiClient);

    await vi.advanceTimersByTimeAsync(1500);
    await flushPromises();
    await vi.advanceTimersByTimeAsync(1500);
    await flushPromises();

    expect(getChatJobConversation).toHaveBeenCalledTimes(2);
  });

  it("clears polling when the job page unmounts", async () => {
    vi.useFakeTimers();
    const getChatJobConversation = vi
      .fn()
      .mockResolvedValue(conversation({ status: "running" }));
    const { wrapper } = await mountJobChatPage({
      getChatJobConversation,
    } as unknown as ApiClient);

    wrapper.unmount();
    await vi.advanceTimersByTimeAsync(1500);
    await flushPromises();

    expect(getChatJobConversation).toHaveBeenCalledTimes(1);
  });
});
