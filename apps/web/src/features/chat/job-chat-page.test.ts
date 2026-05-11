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
  item: ChatJobConversation,
  initialPath = "/chat/jobs/job-1",
) {
  setApiClient(makeApiClient(item));

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

  return { router, wrapper };
}

afterEach(() => {
  resetApiClient();
  vi.restoreAllMocks();
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
});
