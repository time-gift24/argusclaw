import { ref, type Ref, computed } from "vue";
import {
  getApiClient,
  type ChatSessionPayload,
  type LlmProviderRecord,
  type AgentRecord,
} from "@/lib/api";

export interface UseChatComposerOptions {
  activeSessionId: Ref<string>;
  activeThreadId: Ref<string>;
  activeBinding: Ref<import("@/lib/api").ChatThreadBinding | null>;
  selectedTemplateId: Ref<number | null>;
  selectedProviderId: Ref<number | null>;
  selectedModel: Ref<string>;
  providers: Ref<LlmProviderRecord[]>;
  templates: Ref<AgentRecord[]>;
  sessionName: Ref<string>;
  threadTitle: Ref<string>;
  threads: Ref<import("@/lib/api").ChatThreadSummary[]>;
  refreshSessions: () => Promise<void>;
  refreshThreads: (sessionId?: string) => Promise<import("@/lib/api").ChatThreadSummary[]>;
  applyChatSessionPayload: (payload: ChatSessionPayload) => void;
  openThreadEvents: (sessionId: string, threadId: string) => void;
  closeThreadEvents: () => void;
  resetRuntimeActivity: () => void;
  refreshStreamUntilSettled: (assistantCountBeforeSend: number) => Promise<void>;
  countAssistantMessages: () => number;
  clearPendingAssistant: () => void;
  streaming: Ref<boolean>;
  assistantCountAtStreamStart: Ref<number>;
  messages: Ref<import("@/lib/api").ChatMessageRecord[]>;
}

export function useChatComposer(options: UseChatComposerOptions) {
  const {
    activeSessionId,
    activeThreadId,
    activeBinding,
    selectedTemplateId,
    selectedProviderId,
    selectedModel,
    providers,
    templates,
    sessionName,
    threadTitle,
    threads,
    refreshSessions,
    refreshThreads,
    applyChatSessionPayload,
    openThreadEvents,
    closeThreadEvents,
    resetRuntimeActivity,
    refreshStreamUntilSettled,
    countAssistantMessages,
    clearPendingAssistant,
    streaming,
    assistantCountAtStreamStart,
    messages,
  } = options;

  const draftMessage = ref("");
  const sending = ref(false);
  const actionMessage = ref("");
  const error = ref("");

  const hasActiveThread = computed(() => Boolean(activeSessionId.value && activeThreadId.value));
  const canMaterializeThread = computed(() => Boolean(selectedTemplateId.value));
  const canSendMessage = computed(() => hasActiveThread.value || canMaterializeThread.value);
  const activeProvider = computed(() => providers.value.find((p) => p.id === Number(selectedProviderId.value)) ?? null);
  const selectedTemplate = computed(() => templates.value.find((t) => t.id === Number(selectedTemplateId.value)) ?? null);

  const senderPlaceholder = computed(() => {
    if (!hasActiveThread.value) {
      return "输入第一条消息，将按当前模板创建新对话";
    }
    if (sending.value) {
      return "正在提交消息，可随时取消";
    }
    return "输入消息，Enter 发送";
  });

  function setError(reason: unknown) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  }

  function normalizedSessionName() {
    return sessionName.value.trim() || "新的 Web 对话";
  }

  async function ensureActiveChatThread(): Promise<{ sessionId: string; threadId: string }> {
    const api = getApiClient();
    if (activeSessionId.value && activeThreadId.value) {
      return {
        sessionId: activeSessionId.value,
        threadId: activeThreadId.value,
      };
    }

    if (!selectedTemplateId.value) {
      throw new Error("请先选择智能体模板。");
    }

    const providerId = selectedProviderId.value === null ? null : Number(selectedProviderId.value);
    const payload = await api.createChatSessionWithThread!({
      name: normalizedSessionName(),
      template_id: Number(selectedTemplateId.value),
      provider_id: Number.isFinite(providerId) ? providerId : null,
      model: selectedModel.value || activeProvider.value?.default_model || null,
    });
    applyChatSessionPayload(payload);
    await refreshSessions();
    await refreshThreads();

    return {
      sessionId: payload.session_id,
      threadId: payload.thread_id,
    };
  }

  async function sendMessage(value: string) {
    const api = getApiClient();
    const content = value.trim();
    if (!content || sending.value) return;

    sending.value = true;
    actionMessage.value = "";
    error.value = "";
    closeThreadEvents();
    resetRuntimeActivity();

    const assistantCountBeforeSend = countAssistantMessages();
    assistantCountAtStreamStart.value = assistantCountBeforeSend;
    streaming.value = true;
    clearPendingAssistant();
    const previousMessages = messages.value;
    let openedThreadEvents = false;
    messages.value = [...previousMessages, createLocalMessage("user", content)];
    try {
      const target = await ensureActiveChatThread();
      openThreadEvents(target.sessionId, target.threadId);
      openedThreadEvents = true;
      await api.sendChatMessage!(target.sessionId, target.threadId, content);
      draftMessage.value = "";
      if (!api.subscribeChatThread) {
        void refreshStreamUntilSettled(assistantCountBeforeSend);
      }
      actionMessage.value = "消息已提交，正在等待流式结果。";
    } catch (reason) {
      messages.value = previousMessages;
      if (openedThreadEvents) {
        closeThreadEvents();
      }
      streaming.value = false;
      clearPendingAssistant();
      setError(reason);
      actionMessage.value = "";
    } finally {
      sending.value = false;
    }
  }

  async function cancelThread() {
    const api = getApiClient();
    if (!activeSessionId.value || !activeThreadId.value) return;
    try {
      closeThreadEvents();
      await api.cancelChatThread!(activeSessionId.value, activeThreadId.value);
      actionMessage.value = "已请求取消当前线程。";
    } catch (reason) {
      setError(reason);
    }
  }

  function createLocalMessage(role: import("@/lib/api").ChatMessageRecord["role"], content: string): import("@/lib/api").ChatMessageRecord {
    return {
      role,
      content,
      reasoning_content: null,
      content_parts: [],
      tool_call_id: null,
      name: null,
      tool_calls: null,
      metadata: null,
    };
  }

  return {
    draftMessage,
    sending,
    actionMessage,
    error,
    senderPlaceholder,
    canMaterializeThread,
    canSendMessage,
    activeProvider,
    selectedTemplate,
    sendMessage,
    cancelThread,
    ensureActiveChatThread,
  };
}
