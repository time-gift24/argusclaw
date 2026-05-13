import { afterEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";

import {
  resetApiClient,
  setApiClient,
  type ApiClient,
  type ChatMessageRecord,
  type ChatThreadSnapshot,
} from "@/lib/api";
import { useChatThreadStream } from "./useChatThreadStream";

function message(content: string): ChatMessageRecord {
  return {
    role: "assistant",
    content,
    reasoning_content: null,
    content_parts: [],
    tool_call_id: null,
    name: null,
    tool_calls: null,
    metadata: null,
  };
}

function snapshot(sessionId: string, threadId: string, messages: ChatMessageRecord[]): ChatThreadSnapshot {
  return {
    session_id: sessionId,
    thread_id: threadId,
    messages,
    turn_count: messages.length,
    token_count: 0,
    plan_item_count: 0,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}

afterEach(() => {
  resetApiClient();
});

describe("useChatThreadStream", () => {
  it("ignores stale refresh results from a previously active thread", async () => {
    const activeSessionId = ref("session-1");
    const activeThreadId = ref("thread-1");
    const oldSnapshot = deferred<ChatThreadSnapshot>();
    const oldMessages = deferred<ChatMessageRecord[]>();
    const newSnapshot = deferred<ChatThreadSnapshot>();
    const newMessages = deferred<ChatMessageRecord[]>();

    setApiClient({
      getHealth: vi.fn(),
      getBootstrap: vi.fn(),
      getRuntimeState: vi.fn(),
      listProviders: vi.fn(),
      saveProvider: vi.fn(),
      listTemplates: vi.fn(),
      saveTemplate: vi.fn(),
      listMcpServers: vi.fn(),
      saveMcpServer: vi.fn(),
      getChatThreadSnapshot: vi.fn().mockImplementation(async (sessionId: string, threadId: string) => {
        if (sessionId === "session-1" && threadId === "thread-1") {
          return oldSnapshot.promise;
        }
        return newSnapshot.promise;
      }),
      listChatMessages: vi.fn().mockImplementation(async (sessionId: string, threadId: string) => {
        if (sessionId === "session-1" && threadId === "thread-1") {
          return oldMessages.promise;
        }
        return newMessages.promise;
      }),
    } as ApiClient);

    const stream = useChatThreadStream({ activeSessionId, activeThreadId });

    const firstRefresh = stream.refreshActiveThread();
    activeSessionId.value = "session-2";
    activeThreadId.value = "thread-2";
    const secondRefresh = stream.refreshActiveThread();

    newSnapshot.resolve(snapshot("session-2", "thread-2", [message("新线程快照")]));
    newMessages.resolve([message("新线程回复")]);
    await secondRefresh;

    expect(stream.messages.value.map((item) => item.content)).toEqual(["新线程回复"]);

    oldSnapshot.resolve(snapshot("session-1", "thread-1", [message("旧线程快照")]));
    oldMessages.resolve([message("旧线程回复")]);
    await firstRefresh;

    expect(stream.messages.value.map((item) => item.content)).toEqual(["新线程回复"]);
  });

  it("surfaces background job events as runtime activities for the active chat", () => {
    const activeSessionId = ref("session-1");
    const activeThreadId = ref("thread-1");
    const stream = useChatThreadStream({ activeSessionId, activeThreadId });

    stream.handleThreadEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "job_runtime_started",
        job_id: "job-42",
      },
    });
    stream.handleThreadEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "job_result",
        job_id: "job-42",
        success: true,
        cancelled: false,
        message: "后台任务完成",
      },
    });

    expect(stream.runtimeActivities.value).toEqual([
      expect.objectContaining({
        id: "job-42",
        kind: "job",
        name: "后台 Job job-42",
        status: "success",
        resultPreview: "后台任务完成",
      }),
    ]);
  });

  it("keeps pending turn timeline ordered across reasoning and tool events", () => {
    const activeSessionId = ref("session-1");
    const activeThreadId = ref("thread-1");
    const stream = useChatThreadStream({ activeSessionId, activeThreadId });

    stream.handleThreadEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "reasoning_delta",
        delta: "先查看目录。",
      },
    });
    stream.handleThreadEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "tool_started",
        tool_call_id: "call-shell",
        tool_name: "shell",
        arguments: { cmd: "pwd" },
      },
    });
    stream.handleThreadEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "tool_completed",
        tool_call_id: "call-shell",
        tool_name: "shell",
        result: "/workspace/project",
        is_error: false,
      },
    });
    stream.handleThreadEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "reasoning_delta",
        delta: "再搜索 runtime。",
      },
    });
    stream.handleThreadEvent({
      session_id: "session-1",
      thread_id: "thread-1",
      turn_number: null,
      payload: {
        type: "tool_started",
        tool_call_id: "call-search",
        tool_name: "mcp.search",
        arguments: { q: "runtime" },
      },
    });

    expect(stream.pendingTimeline.value).toEqual([
      {
        type: "reasoning",
        id: "pending-reasoning-0",
        text: "先查看目录。",
      },
      expect.objectContaining({
        type: "tool_call",
        id: "call-shell",
        status: "success",
        inputPreview: "{\n  \"cmd\": \"pwd\"\n}",
        outputPreview: "/workspace/project",
      }),
      {
        type: "reasoning",
        id: "pending-reasoning-2",
        text: "再搜索 runtime。",
      },
      expect.objectContaining({
        type: "tool_call",
        id: "call-search",
        status: "running",
        inputPreview: "{\n  \"q\": \"runtime\"\n}",
      }),
    ]);

    stream.clearPendingAssistant();

    expect(stream.pendingTimeline.value).toEqual([]);
  });
});
