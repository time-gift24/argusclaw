import test, { beforeEach } from "node:test";
import assert from "node:assert/strict";

import type { ChatSessionState } from "../lib/chat-store";
import type {
  JobDetailPayload,
  ThreadEventEnvelope,
} from "../lib/types/chat";

const localStorageStub = {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
};

Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: {
    __TAURI_INTERNALS__: {
      invoke: async () => null,
      transformCallback: () => 0,
    },
    localStorage: localStorageStub,
    matchMedia: () => ({
      matches: false,
      addEventListener: () => {},
      removeEventListener: () => {},
    }),
  },
});

Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: localStorageStub,
});

Object.defineProperty(globalThis, "document", {
  configurable: true,
  value: {
    documentElement: {
      classList: {
        add: () => {},
        remove: () => {},
        contains: () => false,
        toggle: () => {},
      },
    },
  },
});

const { useChatStore } = await import("../lib/chat-store");

function makeDetail(
  overrides: Partial<JobDetailPayload> & Pick<JobDetailPayload, "job_id">,
): JobDetailPayload {
  return {
    job_id: overrides.job_id,
    agent_id: overrides.agent_id ?? 101,
    agent_display_name: overrides.agent_display_name ?? "Worker",
    agent_description: overrides.agent_description ?? "Background worker",
    prompt: overrides.prompt ?? "Investigate the issue",
    status: overrides.status ?? "running",
    summary_text: overrides.summary_text ?? "Summary",
    result_text: overrides.result_text ?? null,
    started_at: overrides.started_at ?? "2026-04-08T00:00:00.000Z",
    finished_at: overrides.finished_at ?? null,
    input_tokens: overrides.input_tokens ?? null,
    output_tokens: overrides.output_tokens ?? null,
    source_message_id: overrides.source_message_id ?? null,
    thread_id: overrides.thread_id ?? "thread-parent",
    timeline: overrides.timeline ?? [],
  };
}

function makeSession(overrides: Partial<ChatSessionState> & Pick<ChatSessionState, "sessionKey" | "sessionId" | "threadId">): ChatSessionState {
  return {
    sessionKey: overrides.sessionKey,
    sessionId: overrides.sessionId,
    templateId: overrides.templateId ?? 1,
    threadId: overrides.threadId,
    effectiveProviderId: overrides.effectiveProviderId ?? null,
    effectiveModel: overrides.effectiveModel ?? null,
    status: overrides.status ?? "idle",
    messages: overrides.messages ?? [],
    pendingUserMessage: overrides.pendingUserMessage ?? null,
    pendingAssistant: overrides.pendingAssistant ?? null,
    jobStatuses: overrides.jobStatuses ?? {},
    jobDetails: overrides.jobDetails ?? {},
    selectedJobDetailId: overrides.selectedJobDetailId ?? null,
    error: overrides.error ?? null,
    tokenCount: overrides.tokenCount ?? 0,
    contextWindow: overrides.contextWindow ?? null,
  };
}

function resetStore() {
  useChatStore.setState({
    activeSessionKey: null,
    errorMessage: null,
    selectedTemplateId: null,
    selectedProviderPreferenceId: null,
    selectedModelOverride: null,
    sessionsByKey: {},
    sessionList: [],
    sessionListLoading: false,
    threadListBySessionId: {},
    threadListLoadingBySessionId: {},
    chatThreadPoolSnapshot: null,
    jobRuntimeSnapshot: null,
    threadPoolSnapshot: null,
    threadPoolSnapshotLoading: false,
    threadPoolError: null,
    threadPoolThreads: [],
    stoppingJobIds: {},
  });
}

beforeEach(() => {
  resetStore();
});

test("fast-path thread switch clears the selected job detail", async () => {
  useChatStore.setState({
    activeSessionKey: "session-1",
    sessionsByKey: {
      "session-1": makeSession({
        sessionKey: "session-1",
        sessionId: "session-1",
        threadId: "thread-1",
        jobDetails: {
          "job-1": makeDetail({ job_id: "job-1" }),
        },
      }),
    },
  });

  useChatStore.getState().openJobDetails("job-1");
  assert.equal(
    useChatStore.getState().sessionsByKey["session-1"].selectedJobDetailId,
    "job-1",
  );

  await useChatStore.getState().switchToThread("session-1", "thread-1");

  assert.equal(
    useChatStore.getState().sessionsByKey["session-1"].selectedJobDetailId,
    null,
  );
});

test("thread switch preserves session-scoped job state for later runtime events", async () => {
  const tauriInternals = (globalThis as any).window.__TAURI_INTERNALS__;
  const originalInvoke = tauriInternals.invoke;
  tauriInternals.invoke = async (command: string) => {
    if (command === "activate_existing_thread") {
      return {
        session_key: "session-1",
        session_id: "session-1",
        template_id: 1,
        thread_id: "thread-2",
        effective_provider_id: null,
        effective_model: null,
      };
    }
    if (command === "get_thread_snapshot") {
      return {
        session_id: "session-1",
        thread_id: "thread-2",
        messages: [],
        turn_count: 2,
        token_count: 11,
        plan_item_count: 0,
      };
    }
    return null;
  };

  try {
    useChatStore.setState({
      activeSessionKey: "session-1",
      sessionsByKey: {
        "session-1": makeSession({
          sessionKey: "session-1",
          sessionId: "session-1",
          threadId: "thread-1",
          jobStatuses: {
            "job-1": {
              job_id: "job-1",
              agent_id: 101,
              prompt: "Investigate the issue",
              status: "running",
              message: null,
              agent_display_name: "Worker",
              agent_description: "Background worker",
            },
          },
          jobDetails: {
            "job-1": makeDetail({ job_id: "job-1" }),
          },
          selectedJobDetailId: "job-1",
        }),
      },
    });

    await useChatStore.getState().switchToThread("session-1", "thread-2");

    const nextSession = useChatStore.getState().sessionsByKey["session-1"];
    assert.equal(nextSession.threadId, "thread-2");
    assert.equal(nextSession.jobStatuses["job-1"]?.status, "running");
    assert.equal(nextSession.jobDetails["job-1"]?.job_id, "job-1");
    assert.equal(nextSession.selectedJobDetailId, null);
  } finally {
    tauriInternals.invoke = originalInvoke;
  }
});

test("job runtime events append the runtime timeline to the matching parent session", () => {
  useChatStore.setState({
    activeSessionKey: "session-parent",
    sessionsByKey: {
      "session-parent": makeSession({
        sessionKey: "session-parent",
        sessionId: "session-parent",
        threadId: "thread-parent",
        jobDetails: {
          "job-1": makeDetail({
            job_id: "job-1",
            timeline: [
              {
                kind: "dispatched",
                at: "2026-04-08T00:00:00.000Z",
                label: "已派发",
                status: "running",
              },
            ],
          }),
        },
      }),
      "session-other": makeSession({
        sessionKey: "session-other",
        sessionId: "session-other",
        threadId: "thread-other",
        jobDetails: {
          "job-1": makeDetail({
            job_id: "job-1",
            timeline: [
              {
                kind: "dispatched",
                at: "2026-04-08T00:00:00.000Z",
                label: "已派发",
                status: "running",
              },
            ],
          }),
        },
      }),
    },
  });

  const envelope: ThreadEventEnvelope = {
    session_id: "session-parent",
    thread_id: "runtime-thread-1",
    turn_number: null,
    payload: {
      type: "job_runtime_started",
      job_id: "job-1",
    },
  };

  useChatStore.getState()._handleThreadEvent(envelope);

  const parentDetail = useChatStore.getState().sessionsByKey["session-parent"]
    .jobDetails["job-1"];
  const otherDetail = useChatStore.getState().sessionsByKey["session-other"]
    .jobDetails["job-1"];

  assert.equal(parentDetail.timeline.length, 2);
  assert.equal(parentDetail.timeline[1].label, "运行中");
  assert.equal(parentDetail.timeline[1].status, "running");
  assert.equal(otherDetail.timeline.length, 1);
  assert.equal(
    useChatStore
      .getState()
      .threadPoolThreads.some(
        (thread) =>
          thread.threadId === "runtime-thread-1" &&
          thread.jobId === "job-1" &&
          thread.status === "running",
      ),
    true,
  );
});

test("job result events still resolve by session id after the active thread changes", () => {
  useChatStore.setState({
    activeSessionKey: "session-parent",
    sessionsByKey: {
      "session-parent": makeSession({
        sessionKey: "session-parent",
        sessionId: "session-parent",
        threadId: "thread-current",
        jobStatuses: {
          "job-1": {
            job_id: "job-1",
            agent_id: 101,
            prompt: "Investigate the issue",
            status: "running",
            message: null,
            agent_display_name: "Worker",
            agent_description: "Background worker",
          },
        },
        jobDetails: {
          "job-1": makeDetail({ job_id: "job-1" }),
        },
      }),
    },
  });

  const envelope: ThreadEventEnvelope = {
    session_id: "session-parent",
    thread_id: "thread-originating",
    turn_number: null,
    payload: {
      type: "job_result",
      job_id: "job-1",
      success: true,
      cancelled: false,
      message: "completed work",
      input_tokens: 12,
      output_tokens: 6,
      agent_id: 101,
      agent_display_name: "Worker",
      agent_description: "Background worker",
    },
  };

  useChatStore.getState()._handleThreadEvent(envelope);

  const session = useChatStore.getState().sessionsByKey["session-parent"];
  assert.equal(session.jobStatuses["job-1"]?.status, "completed");
  assert.equal(session.jobDetails["job-1"]?.result_text, "completed work");
  assert.equal(session.jobDetails["job-1"]?.timeline.at(-1)?.status, "completed");
});
