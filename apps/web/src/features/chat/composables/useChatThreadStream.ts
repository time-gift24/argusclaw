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
  name: string;
  status: ToolActivityStatus;
  argumentsPreview: string;
  resultPreview: string;
}

export interface UseChatThreadStreamOptions {
  activeSessionId: Ref<string>;
  activeThreadId: Ref<string>;
}

export function useChatThreadStream(options: UseChatThreadStreamOptions) {
  const { activeSessionId, activeThreadId } = options;

  const messages: Ref<ChatMessageRecord[]> = ref<ChatMessageRecord[]>([]);
  const streaming = ref(false);
  const pendingAssistantContent = ref("");
  const pendingAssistantReasoning = ref("");
  const runtimeActivities: Ref<ToolActivity[]> = ref<ToolActivity[]>([]);
  const runtimeNotice = ref("");
  const threadLoading = ref(false);
  const assistantCountAtStreamStart = ref(0);
  const streamSubscription = ref<RuntimeEventSubscription | null>(null);

  function openThreadEvents(sessionId: string, threadId: string) {
    const api = getApiClient();
    closeThreadEvents();
    if (!api.subscribeChatThread) return;

    try {
      streamSubscription.value = api.subscribeChatThread(sessionId, threadId, {
        onEvent: handleThreadEvent,
        onError: (reason) => {
          closeThreadEvents();
          if (streaming.value) {
            runtimeNotice.value = reason.message;
            void refreshStreamUntilSettled(countAssistantMessages());
          }
        },
      });
    } catch (reason) {
      runtimeNotice.value = reason instanceof Error ? reason.message : String(reason);
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
        break;
      case "reasoning_delta":
        streaming.value = true;
        pendingAssistantReasoning.value += payload.delta;
        break;
      case "retry_attempt":
        runtimeNotice.value = `正在重试第 ${payload.attempt}/${payload.max_retries} 次：${payload.error}`;
        break;
      case "tool_started":
        streaming.value = true;
        upsertToolActivity({
          id: payload.tool_call_id,
          name: payload.tool_name,
          status: "running",
          argumentsPreview: previewValue(payload.arguments),
          resultPreview: "",
        });
        if (!pendingAssistantContent.value) {
          pendingAssistantContent.value = `正在调用工具：${payload.tool_name}`;
        }
        break;
      case "tool_completed":
        upsertToolActivity({
          id: payload.tool_call_id,
          name: payload.tool_name,
          status: payload.is_error ? "error" : "success",
          argumentsPreview: "",
          resultPreview: previewValue(payload.result),
        });
        break;
      case "turn_failed":
        streaming.value = false;
        runtimeNotice.value = `运行失败：${payload.error}`;
        clearPendingAssistant();
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
      runtimeActivities.value = [nextActivity, ...runtimeActivities.value];
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

  function resetRuntimeActivity() {
    runtimeActivities.value = [];
    runtimeNotice.value = "";
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
      messages.value = nextMessages.length > 0 ? nextMessages : snapshot.messages;
    } catch (reason) {
      runtimeNotice.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (!options.silent) {
        threadLoading.value = false;
      }
    }
  }

  return {
    messages,
    streaming,
    pendingAssistantContent,
    pendingAssistantReasoning,
    runtimeActivities,
    runtimeNotice,
    threadLoading,
    assistantCountAtStreamStart,
    openThreadEvents,
    closeThreadEvents,
    handleThreadEvent,
    refreshActiveThread,
    resetRuntimeActivity,
    countAssistantMessages,
    clearPendingAssistant,
    refreshStreamUntilSettled,
    waitForStreamTick,
  };
}
