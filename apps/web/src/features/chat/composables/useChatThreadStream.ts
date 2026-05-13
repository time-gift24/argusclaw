import { ref, type Ref } from "vue";
import {
  getApiClient,
  type ChatMessageRecord,
  type ChatThreadEventEnvelope,
  type ChatThreadEventPayload,
  type RuntimeEventSubscription,
} from "@/lib/api";

export type ToolActivityStatus = "running" | "success" | "error";

export interface ToolActivity {
  id: string;
  kind?: "job" | "tool";
  name: string;
  status: ToolActivityStatus;
  argumentsPreview: string;
  resultPreview: string;
}

export type TurnTimelineItem =
  | {
      type: "reasoning";
      id: string;
      text: string;
    }
  | {
      type: "tool_call";
      id: string;
      kind: "shell" | "mcp" | "search" | "http" | "file" | "job" | "tool";
      name: string;
      status: ToolActivityStatus;
      inputPreview: string;
      outputPreview: string;
    };

export interface UseChatThreadStreamOptions {
  activeSessionId: Ref<string>;
  activeThreadId: Ref<string>;
}

interface ThreadTransientState {
  streaming: boolean;
  pendingAssistantContent: string;
  pendingAssistantReasoning: string;
  pendingTimeline: TurnTimelineItem[];
  runtimeActivities: ToolActivity[];
  runtimeNotice: string;
  assistantCountAtStreamStart: number;
  messages: ChatMessageRecord[];
}

export function useChatThreadStream(options: UseChatThreadStreamOptions) {
  const { activeSessionId, activeThreadId } = options;
  let refreshRequestId = 0;
  const transientStateByThread = new Map<string, ThreadTransientState>();

  const messages: Ref<ChatMessageRecord[]> = ref<ChatMessageRecord[]>([]);
  const streaming = ref(false);
  const pendingAssistantContent = ref("");
  const pendingAssistantReasoning = ref("");
  const pendingTimeline: Ref<TurnTimelineItem[]> = ref<TurnTimelineItem[]>([]);
  const runtimeActivities: Ref<ToolActivity[]> = ref<ToolActivity[]>([]);
  const runtimeNotice = ref("");
  const threadLoading = ref(false);
  const assistantCountAtStreamStart = ref(0);
  const streamSubscription = ref<RuntimeEventSubscription | null>(null);

  function openThreadEvents(sessionId: string, threadId: string): Promise<void> {
    const api = getApiClient();
    closeThreadEvents();
    const subscribeChatThread = api.subscribeChatThread?.bind(api);
    if (!subscribeChatThread) return Promise.resolve();

    try {
      return new Promise((resolve) => {
        let ready = false;
        const markReady = () => {
          if (ready) return;
          ready = true;
          resolve();
        };

        streamSubscription.value = subscribeChatThread(sessionId, threadId, {
          onOpen: markReady,
          onEvent: (event) => {
            markReady();
            handleThreadEvent(event);
          },
          onError: (reason) => {
            markReady();
            closeThreadEvents();
            if (streaming.value) {
              runtimeNotice.value = reason.message;
              void refreshStreamUntilSettled(countAssistantMessages());
            }
          },
        });

        if (import.meta.env.MODE === "test") {
          markReady();
          return;
        }

        window.setTimeout(markReady, 800);
      });
    } catch (reason) {
      runtimeNotice.value = reason instanceof Error ? reason.message : String(reason);
      return Promise.resolve();
    }
  }

  function closeThreadEvents() {
    streamSubscription.value?.close();
    streamSubscription.value = null;
  }

  function handleThreadEvent(event: ChatThreadEventEnvelope) {
    if (event.session_id !== activeSessionId.value || event.thread_id !== activeThreadId.value) {
      return;
    }
    applyThreadEventPayload(event.payload);
  }

  function applyThreadEventPayload(payload: ChatThreadEventPayload) {
    switch (payload.type) {
      case "content_delta":
        streaming.value = true;
        pendingAssistantContent.value += payload.delta;
        saveActiveThreadTransientState();
        break;
      case "reasoning_delta":
        streaming.value = true;
        pendingAssistantReasoning.value += payload.delta;
        appendPendingReasoning(payload.delta);
        saveActiveThreadTransientState();
        break;
      case "retry_attempt":
        runtimeNotice.value = `正在重试第 ${payload.attempt}/${payload.max_retries} 次：${payload.error}`;
        saveActiveThreadTransientState();
        break;
      case "tool_started":
        streaming.value = true;
        upsertToolActivity({
          id: payload.tool_call_id,
          kind: "tool",
          name: payload.tool_name,
          status: "running",
          argumentsPreview: previewValue(payload.arguments),
          resultPreview: "",
        });
        upsertPendingToolTimeline({
          type: "tool_call",
          id: payload.tool_call_id,
          kind: toolKind(payload.tool_name),
          name: payload.tool_name,
          status: "running",
          inputPreview: previewValue(payload.arguments),
          outputPreview: "",
        });
        saveActiveThreadTransientState();
        break;
      case "tool_completed":
        upsertToolActivity({
          id: payload.tool_call_id,
          kind: "tool",
          name: payload.tool_name,
          status: payload.is_error ? "error" : "success",
          argumentsPreview: "",
          resultPreview: previewValue(payload.result),
        });
        upsertPendingToolTimeline({
          type: "tool_call",
          id: payload.tool_call_id,
          kind: toolKind(payload.tool_name),
          name: payload.tool_name,
          status: payload.is_error ? "error" : "success",
          inputPreview: "",
          outputPreview: previewValue(payload.result),
        });
        saveActiveThreadTransientState();
        break;
      case "job_dispatched":
        streaming.value = true;
        upsertJobActivity(payload.job_id, {
          status: "running",
          argumentsPreview: typeof payload.prompt === "string" ? payload.prompt : "后台任务已派发",
          resultPreview: "",
        });
        upsertPendingToolTimeline({
          type: "tool_call",
          id: normalizeJobId(payload.job_id),
          kind: "job",
          name: `后台 Job ${normalizeJobId(payload.job_id)}`,
          status: "running",
          inputPreview: typeof payload.prompt === "string" ? payload.prompt : "后台任务已派发",
          outputPreview: "",
        });
        saveActiveThreadTransientState();
        break;
      case "job_runtime_queued":
        streaming.value = true;
        upsertJobActivity(payload.job_id, {
          status: "running",
          argumentsPreview: "等待后台 Job runtime",
          resultPreview: "",
        });
        saveActiveThreadTransientState();
        break;
      case "job_runtime_started":
        streaming.value = true;
        upsertJobActivity(payload.job_id, {
          status: "running",
          argumentsPreview: "后台 Job runtime 已启动",
          resultPreview: "",
        });
        saveActiveThreadTransientState();
        break;
      case "job_runtime_cooling":
      case "job_runtime_evicted":
        upsertJobActivity(payload.job_id, {
          status: "success",
          argumentsPreview: "",
          resultPreview: "后台 Job runtime 已结束",
        });
        saveActiveThreadTransientState();
        break;
      case "job_result":
        upsertJobActivity(payload.job_id, {
          status: payload.success && !payload.cancelled ? "success" : "error",
          argumentsPreview: "",
          resultPreview: typeof payload.message === "string" ? payload.message : previewValue(payload),
        });
        upsertPendingToolTimeline({
          type: "tool_call",
          id: normalizeJobId(payload.job_id),
          kind: "job",
          name: `后台 Job ${normalizeJobId(payload.job_id)}`,
          status: payload.success && !payload.cancelled ? "success" : "error",
          inputPreview: "",
          outputPreview: typeof payload.message === "string" ? payload.message : previewValue(payload),
        });
        saveActiveThreadTransientState();
        break;
      case "turn_failed":
        streaming.value = false;
        runtimeNotice.value = `运行失败：${payload.error}`;
        clearActiveThreadTransientState();
        closeThreadEvents();
        void refreshActiveThread({ silent: true });
        break;
      case "turn_settled":
      case "idle":
        void settleThreadAfterStream();
        break;
      default:
        break;
    }
  }

  function upsertToolActivity(nextActivity: ToolActivity) {
    const existingIndex = runtimeActivities.value.findIndex((item) => item.id === nextActivity.id);
    if (existingIndex === -1) {
      runtimeActivities.value = [...runtimeActivities.value, nextActivity];
      return;
    }
    const existing = runtimeActivities.value[existingIndex];
    runtimeActivities.value = runtimeActivities.value.map((item, index) =>
      index === existingIndex
        ? {
            ...existing,
            ...nextActivity,
            argumentsPreview: nextActivity.argumentsPreview || existing.argumentsPreview,
            resultPreview: nextActivity.resultPreview || existing.resultPreview,
          }
        : item,
    );
  }

  function appendPendingReasoning(delta: string) {
    if (!delta) return;
    const lastItem = pendingTimeline.value[pendingTimeline.value.length - 1];
    if (lastItem?.type === "reasoning") {
      pendingTimeline.value = pendingTimeline.value.map((item, index) =>
        index === pendingTimeline.value.length - 1 && item.type === "reasoning"
          ? { ...item, text: item.text + delta }
          : item,
      );
      return;
    }

    pendingTimeline.value = [
      ...pendingTimeline.value,
      {
        type: "reasoning",
        id: `pending-reasoning-${pendingTimeline.value.length}`,
        text: delta,
      },
    ];
  }

  function upsertPendingToolTimeline(nextItem: Extract<TurnTimelineItem, { type: "tool_call" }>) {
    const existingIndex = pendingTimeline.value.findIndex(
      (item) => item.type === "tool_call" && item.id === nextItem.id,
    );
    if (existingIndex === -1) {
      pendingTimeline.value = [...pendingTimeline.value, nextItem];
      return;
    }

    const existing = pendingTimeline.value[existingIndex];
    pendingTimeline.value = pendingTimeline.value.map((item, index) =>
      index === existingIndex && existing.type === "tool_call"
        ? {
            ...existing,
            ...nextItem,
            inputPreview: nextItem.inputPreview || existing.inputPreview,
            outputPreview: nextItem.outputPreview || existing.outputPreview,
          }
        : item,
    );
  }

  function upsertJobActivity(
    jobId: unknown,
    update: Pick<ToolActivity, "status" | "argumentsPreview" | "resultPreview">,
  ) {
    const normalizedJobId = normalizeJobId(jobId);
    upsertToolActivity({
      id: normalizedJobId,
      kind: "job",
      name: `后台 Job ${normalizedJobId}`,
      ...update,
    });
  }

  function normalizeJobId(jobId: unknown) {
    return typeof jobId === "string" && jobId.trim() ? jobId.trim() : "unknown";
  }

  function resetRuntimeActivity() {
    runtimeActivities.value = [];
    runtimeNotice.value = "";
    saveActiveThreadTransientState();
  }

  function resetTransientState() {
    refreshRequestId += 1;
    streaming.value = false;
    threadLoading.value = false;
    runtimeNotice.value = "";
    clearPendingAssistant();
  }

  function activeThreadCacheKey() {
    return threadCacheKey(activeSessionId.value, activeThreadId.value);
  }

  function threadCacheKey(sessionId: string, threadId: string) {
    return sessionId && threadId ? `${sessionId}:${threadId}` : "";
  }

  function shouldCacheTransientState() {
    return Boolean(
      streaming.value ||
        pendingAssistantContent.value ||
        pendingAssistantReasoning.value ||
        pendingTimeline.value.length > 0 ||
        runtimeActivities.value.length > 0 ||
        runtimeNotice.value ||
        messages.value.length > 0,
    );
  }

  function saveActiveThreadTransientState() {
    const key = activeThreadCacheKey();
    if (!key) return;
    if (!shouldCacheTransientState()) {
      transientStateByThread.delete(key);
      return;
    }
    transientStateByThread.set(key, {
      streaming: streaming.value,
      pendingAssistantContent: pendingAssistantContent.value,
      pendingAssistantReasoning: pendingAssistantReasoning.value,
      pendingTimeline: pendingTimeline.value.map((item) => ({ ...item })),
      runtimeActivities: runtimeActivities.value.map((item) => ({ ...item })),
      runtimeNotice: runtimeNotice.value,
      assistantCountAtStreamStart: assistantCountAtStreamStart.value,
      messages: messages.value.map((item) => ({ ...item })),
    });
  }

  function restoreActiveThreadTransientState() {
    const cached = transientStateByThread.get(activeThreadCacheKey());
    if (!cached) {
      streaming.value = false;
      threadLoading.value = false;
      runtimeNotice.value = "";
      runtimeActivities.value = [];
      clearPendingAssistant();
      messages.value = [];
      assistantCountAtStreamStart.value = 0;
      return;
    }

    streaming.value = cached.streaming;
    threadLoading.value = false;
    pendingAssistantContent.value = cached.pendingAssistantContent;
    pendingAssistantReasoning.value = cached.pendingAssistantReasoning;
    pendingTimeline.value = cached.pendingTimeline.map((item) => ({ ...item }));
    runtimeActivities.value = cached.runtimeActivities.map((item) => ({ ...item }));
    runtimeNotice.value = cached.runtimeNotice;
    assistantCountAtStreamStart.value = cached.assistantCountAtStreamStart;
    messages.value = cached.messages.map((item) => ({ ...item }));
  }

  function clearActiveThreadTransientState() {
    const key = activeThreadCacheKey();
    if (key) transientStateByThread.delete(key);
    streaming.value = false;
    runtimeNotice.value = "";
    runtimeActivities.value = [];
    clearPendingAssistant();
  }

  function previewValue(value: unknown): string {
    if (value === null || value === undefined) return "";
    if (typeof value === "string") return value;
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return String(value);
    }
  }

  async function settleThreadAfterStream() {
    await refreshActiveThread({ silent: true });
    if (countAssistantMessages() <= assistantCountAtStreamStart.value) {
      void refreshStreamUntilSettled(assistantCountAtStreamStart.value);
      return;
    }
    streaming.value = false;
    clearPendingAssistant();
    runtimeNotice.value = "回复已刷新。";
  }

  function countAssistantMessages() {
    return messages.value.filter((m) => m.role === "assistant").length;
  }

  function clearPendingAssistant() {
    pendingAssistantContent.value = "";
    pendingAssistantReasoning.value = "";
    pendingTimeline.value = [];
  }

  function toolKind(name: string): Extract<TurnTimelineItem, { type: "tool_call" }>["kind"] {
    if (name === "shell" || name.startsWith("shell.") || name === "exec") return "shell";
    if (name.startsWith("mcp.")) return "mcp";
    if (name.includes("search")) return "search";
    if (name.includes("http") || name.includes("fetch")) return "http";
    if (name.includes("file") || name.includes("fs")) return "file";
    return "tool";
  }

  async function refreshStreamUntilSettled(assistantCountBeforeSend: number) {
    for (let attempt = 0; attempt < 8; attempt += 1) {
      if (!streaming.value || !activeSessionId.value || !activeThreadId.value) {
        return;
      }
      await refreshActiveThread({ silent: true });
      if (countAssistantMessages() > assistantCountBeforeSend) {
        streaming.value = false;
        clearPendingAssistant();
        runtimeNotice.value = "回复已刷新。";
        return;
      }
      await waitForStreamTick();
    }
    streaming.value = false;
    clearPendingAssistant();
    runtimeNotice.value = "消息已提交，后端仍在处理；可点击刷新获取最新回复。";
  }

  function waitForStreamTick() {
    if (import.meta.env.MODE === "test") {
      return Promise.resolve();
    }
    return new Promise<void>((resolve) => {
      window.setTimeout(resolve, 900);
    });
  }

  async function refreshActiveThread(options: { silent?: boolean } = {}) {
    const api = getApiClient();
    const sessionId = activeSessionId.value;
    const threadId = activeThreadId.value;
    const requestId = ++refreshRequestId;
    if (!sessionId || !threadId) {
      messages.value = [];
      return;
    }
    if (!options.silent) {
      threadLoading.value = true;
    }
    try {
      const [snapshot, nextMessages] = await Promise.all([
        api.getChatThreadSnapshot!(sessionId, threadId),
        api.listChatMessages!(sessionId, threadId),
      ]);
      if (
        requestId !== refreshRequestId ||
        sessionId !== activeSessionId.value ||
        threadId !== activeThreadId.value
      ) {
        return;
      }
      messages.value = nextMessages.length > 0 ? nextMessages : snapshot.messages;
      if (streaming.value && countAssistantMessages() > assistantCountAtStreamStart.value) {
        clearActiveThreadTransientState();
      } else {
        saveActiveThreadTransientState();
      }
    } catch (reason) {
      if (
        requestId !== refreshRequestId ||
        sessionId !== activeSessionId.value ||
        threadId !== activeThreadId.value
      ) {
        return;
      }
      runtimeNotice.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (!options.silent && requestId === refreshRequestId) {
        threadLoading.value = false;
      }
    }
  }

  return {
    messages,
    streaming,
    pendingAssistantContent,
    pendingAssistantReasoning,
    pendingTimeline,
    runtimeActivities,
    runtimeNotice,
    threadLoading,
    assistantCountAtStreamStart,
    openThreadEvents,
    closeThreadEvents,
    handleThreadEvent,
    refreshActiveThread,
    resetRuntimeActivity,
    resetTransientState,
    saveActiveThreadTransientState,
    restoreActiveThreadTransientState,
    clearActiveThreadTransientState,
    countAssistantMessages,
    clearPendingAssistant,
    refreshStreamUntilSettled,
    waitForStreamTick,
  };
}
