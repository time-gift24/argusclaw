<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, ref } from "vue";
import { TrBubbleList, TrPrompts, TrSender, type BubbleRoleConfig, type PromptProps } from "@opentiny/tiny-robot";

import {
  getApiClient,
  type AgentRecord,
  type ApiClient,
  type ChatMessageRecord,
  type ChatSessionPayload,
  type ChatSessionSummary,
  type ChatThreadBinding,
  type ChatThreadEventEnvelope,
  type ChatThreadEventPayload,
  type ChatThreadSummary,
  type LlmProviderRecord,
  type RuntimeEventSubscription,
} from "@/lib/api";
import { TinyButton, TinyInput, TinyOption, TinySelect, TinyTag } from "@/lib/opentiny";

const api = getApiClient();
type ChatApiMethods = Required<
  Pick<
    ApiClient,
    | "listChatSessions"
    | "createChatSessionWithThread"
    | "renameChatSession"
    | "deleteChatSession"
    | "listChatThreads"
    | "createChatThread"
    | "renameChatThread"
    | "deleteChatThread"
    | "getChatThreadSnapshot"
    | "updateChatThreadModel"
    | "activateChatThread"
    | "listChatMessages"
    | "sendChatMessage"
    | "cancelChatThread"
  >
>;

const sessions = ref<ChatSessionSummary[]>([]);
const threads = ref<ChatThreadSummary[]>([]);
const messages = ref<ChatMessageRecord[]>([]);
const providers = ref<LlmProviderRecord[]>([]);
const templates = ref<AgentRecord[]>([]);
const activeSessionId = ref("");
const activeThreadId = ref("");
const draftMessage = ref("");
const sessionName = ref("新的 Web 对话");
const threadTitle = ref("新的对话线程");
const selectedTemplateId = ref<number | null>(null);
const selectedProviderId = ref<number | null>(null);
const selectedModel = ref("");
const loading = ref(true);
const threadLoading = ref(false);
const sending = ref(false);
const creatingThread = ref(false);
const deleting = ref(false);
const streaming = ref(false);
const pendingAssistantContent = ref("");
const pendingAssistantReasoning = ref("");
const assistantCountAtStreamStart = ref(0);
const actionMessage = ref("");
const error = ref("");
const activeBinding = ref<ChatThreadBinding | null>(null);
const streamSubscription = ref<RuntimeEventSubscription | null>(null);

const activeSession = computed(() => sessions.value.find((session) => session.id === activeSessionId.value) ?? null);
const activeThread = computed(() => threads.value.find((thread) => thread.id === activeThreadId.value) ?? null);
const selectedTemplate = computed(() => templates.value.find((item) => item.id === Number(selectedTemplateId.value)) ?? null);
const hasActiveThread = computed(() => Boolean(activeSessionId.value && activeThreadId.value));
const canMaterializeThread = computed(() => Boolean(selectedTemplateId.value));
const canCreateThread = computed(() => Boolean(activeSession.value && selectedTemplateId.value));
const canSendMessage = computed(() => hasActiveThread.value || canMaterializeThread.value);
const activeProvider = computed(
  () => providers.value.find((provider) => provider.id === Number(selectedProviderId.value)) ?? null,
);
const senderPlaceholder = computed(() => {
  if (!hasActiveThread.value) {
    return "输入第一条消息，将按当前模板创建新对话";
  }
  if (sending.value) {
    return "正在提交消息，可随时取消";
  }

  return "输入消息，Enter 发送";
});

const robotMessages = computed(() => {
  const nextMessages = messages.value
    .filter((message) => message.role !== "system")
    .map((message) => ({
      role: message.role,
      content: displayMessageContent(message),
    }));

  if (streaming.value && (hasActiveThread.value || nextMessages.length > 0)) {
    nextMessages.push({
      role: "assistant",
      content: pendingAssistantContent.value || (pendingAssistantReasoning.value ? "正在思考…" : "正在生成回复…"),
    });
  }

  return nextMessages;
});

const stats = computed(() => ({
  sessions: sessions.value.length,
  threads: threads.value.length,
  messages: messages.value.length,
}));
const currentConversationTitle = computed(() => {
  if (activeThread.value) {
    return formatThreadTitle(activeThread.value);
  }

  return hasActiveThread.value ? "新的对话线程" : "新对话草稿";
});

const starterPrompts: PromptProps[] = [
  {
    id: "provider",
    label: "检查模型配置",
    description: "当前默认模型和可用 provider 是否适合这个任务？",
    icon: h("span", { class: "prompt-icon" }, "AI"),
  },
  {
    id: "mcp",
    label: "规划 MCP 运维",
    description: "帮我整理当前 MCP 服务的风险和下一步动作。",
    icon: h("span", { class: "prompt-icon" }, "MCP"),
  },
  {
    id: "template",
    label: "优化智能体模板",
    description: "基于当前模板给出系统提示词改进建议。",
    icon: h("span", { class: "prompt-icon" }, "TPL"),
  },
];

const bubbleRoles: Record<string, BubbleRoleConfig> = {
  assistant: {
    placement: "start",
    avatar: h("span", { class: "chat-avatar chat-avatar--assistant" }, "AI"),
  },
  tool: {
    placement: "start",
    avatar: h("span", { class: "chat-avatar chat-avatar--tool" }, "T"),
  },
  user: {
    placement: "end",
    avatar: h("span", { class: "chat-avatar chat-avatar--user" }, "我"),
  },
};

onMounted(() => {
  void loadInitialState();
});

onBeforeUnmount(() => {
  closeThreadEvents();
});

async function loadInitialState() {
  loading.value = true;
  error.value = "";
  const loadErrors: string[] = [];
  try {
    const [providersResult, templatesResult, sessionsResult] = await Promise.allSettled([
      api.listProviders(),
      api.listTemplates(),
      callChatApi("listChatSessions"),
    ]);

    if (providersResult.status === "fulfilled") {
      providers.value = providersResult.value;
    } else {
      loadErrors.push(`模型提供方加载失败：${errorMessage(providersResult.reason)}`);
    }

    if (templatesResult.status === "fulfilled") {
      templates.value = templatesResult.value;
    } else {
      loadErrors.push(`智能体模板加载失败：${errorMessage(templatesResult.reason)}`);
    }

    selectDefaultTemplateAndProvider();

    if (sessionsResult.status === "fulfilled") {
      const nextSessions = sessionsResult.value;
      sessions.value = nextSessions;
      if (nextSessions.length > 0) {
        try {
          await selectSession(nextSessions[0].id);
        } catch (reason) {
          loadErrors.push(`对话线程加载失败：${errorMessage(reason)}`);
        }
      }
    } else {
      loadErrors.push(`对话会话加载失败：${errorMessage(sessionsResult.reason)}`);
    }

    if (loadErrors.length > 0) {
      error.value = loadErrors.join("；");
    }
  } catch (reason) {
    setError(reason);
  } finally {
    loading.value = false;
  }
}

async function refreshSessions() {
  sessions.value = await callChatApi("listChatSessions");
}

async function selectSession(sessionId: string) {
  streaming.value = false;
  closeThreadEvents();
  activeSessionId.value = sessionId;
  activeThreadId.value = "";
  messages.value = [];
  activeBinding.value = null;
  const session = sessions.value.find((item) => item.id === sessionId);
  if (session) {
    sessionName.value = formatSessionName(session);
  }
  await refreshThreads();
  if (threads.value.length > 0) {
    await selectThread(threads.value[0].id);
  }
}

async function refreshThreads() {
  if (!activeSessionId.value) {
    threads.value = [];
    return;
  }

  threads.value = await callChatApi("listChatThreads", activeSessionId.value);
}

async function selectThread(threadId: string) {
  streaming.value = false;
  closeThreadEvents();
  activeThreadId.value = threadId;
  const thread = threads.value.find((item) => item.id === threadId);
  threadTitle.value = thread?.title ?? "新的对话线程";
  await refreshActiveThread();
  openThreadEvents(activeSessionId.value, threadId);
}

async function refreshActiveThread(options: { silent?: boolean } = {}) {
  if (!activeSessionId.value || !activeThreadId.value) {
    messages.value = [];
    return;
  }

  if (!options.silent) {
    threadLoading.value = true;
  }
  try {
    const [snapshot, nextMessages] = await Promise.all([
      callChatApi("getChatThreadSnapshot", activeSessionId.value, activeThreadId.value),
      callChatApi("listChatMessages", activeSessionId.value, activeThreadId.value),
    ]);
    messages.value = nextMessages.length > 0 ? nextMessages : snapshot.messages;
  } catch (reason) {
    setError(reason);
  } finally {
    if (!options.silent) {
      threadLoading.value = false;
    }
  }
}

function startNewChatDraft() {
  streaming.value = false;
  pendingAssistantContent.value = "";
  pendingAssistantReasoning.value = "";
  closeThreadEvents();
  activeSessionId.value = "";
  activeThreadId.value = "";
  activeBinding.value = null;
  messages.value = [];
  threads.value = [];
  sessionName.value = "新的 Web 对话";
  threadTitle.value = "新的对话线程";
  actionMessage.value = "已准备新对话草稿，发送第一条消息后会创建会话和线程。";
  error.value = "";
}

async function renameSession() {
  if (!activeSessionId.value) {
    return;
  }
  await runAction(async () => {
    const renamed = await callChatApi("renameChatSession", activeSessionId.value, sessionName.value);
    sessions.value = sessions.value.map((session) => (session.id === renamed.id ? renamed : session));
  }, "会话名称已更新。");
}

async function deleteSession() {
  if (!activeSessionId.value || deleting.value) {
    return;
  }
  deleting.value = true;
  try {
    await runAction(async () => {
      await callChatApi("deleteChatSession", activeSessionId.value);
      await refreshSessions();
      activeSessionId.value = "";
      activeThreadId.value = "";
      threads.value = [];
      messages.value = [];
      if (sessions.value.length > 0) {
        await selectSession(sessions.value[0].id);
      }
    }, "会话已删除。");
  } finally {
    deleting.value = false;
  }
}

async function createThread() {
  if (!activeSessionId.value || !selectedTemplateId.value) {
    setError("请先选择会话和智能体模板。");
    return;
  }

  creatingThread.value = true;
  try {
    await runAction(async () => {
      const templateId = Number(selectedTemplateId.value);
      const providerId = selectedProviderId.value === null ? null : Number(selectedProviderId.value);
      const created = await callChatApi("createChatThread", activeSessionId.value, {
        template_id: templateId,
        provider_id: Number.isFinite(providerId) ? providerId : null,
        model: selectedModel.value || activeProvider.value?.default_model || null,
      });
      await refreshThreads();
      if (!threads.value.some((thread) => thread.id === created.id)) {
        threads.value = [created, ...threads.value];
      }
      await selectThread(created.id);
    }, "线程已创建，可以开始对话。");
  } finally {
    creatingThread.value = false;
  }
}

async function renameThread() {
  if (!activeSessionId.value || !activeThreadId.value) {
    return;
  }
  await runAction(async () => {
    const renamed = await callChatApi("renameChatThread", activeSessionId.value, activeThreadId.value, threadTitle.value);
    threads.value = threads.value.map((thread) => (thread.id === renamed.id ? renamed : thread));
  }, "线程标题已更新。");
}

async function deleteThread() {
  if (!activeSessionId.value || !activeThreadId.value || deleting.value) {
    return;
  }
  deleting.value = true;
  try {
    await runAction(async () => {
      await callChatApi("deleteChatThread", activeSessionId.value, activeThreadId.value);
      await refreshThreads();
      activeThreadId.value = "";
      messages.value = [];
      if (threads.value.length > 0) {
        await selectThread(threads.value[0].id);
      }
    }, "线程已删除。");
  } finally {
    deleting.value = false;
  }
}

async function applyModelBinding() {
  if (!activeSessionId.value || !activeThreadId.value || !selectedProviderId.value || !selectedModel.value) {
    setError("请选择线程、提供方和模型后再应用。");
    return;
  }

  await runAction(async () => {
    const providerId = Number(selectedProviderId.value);
    activeBinding.value = await callChatApi("updateChatThreadModel", activeSessionId.value, activeThreadId.value, {
      provider_id: providerId,
      model: selectedModel.value,
    });
  }, "模型绑定已更新。");
}

async function activateThread() {
  if (!activeSessionId.value || !activeThreadId.value) {
    return;
  }
  await runAction(async () => {
    activeBinding.value = await callChatApi("activateChatThread", activeSessionId.value, activeThreadId.value);
  }, "线程已激活。");
}

async function sendMessage(value: string) {
  const content = value.trim();
  if (!content || sending.value) {
    return;
  }

  sending.value = true;
  streaming.value = true;
  pendingAssistantContent.value = "";
  pendingAssistantReasoning.value = "";
  const assistantCountBeforeSend = countAssistantMessages();
  assistantCountAtStreamStart.value = assistantCountBeforeSend;
  messages.value = [...messages.value, createLocalMessage("user", content)];
  try {
    await runAction(async () => {
      const target = await ensureActiveChatThread();
      openThreadEvents(target.sessionId, target.threadId);
      await callChatApi("sendChatMessage", target.sessionId, target.threadId, content);
      draftMessage.value = "";
      if (!streamSubscription.value) {
        void refreshStreamUntilSettled(assistantCountBeforeSend);
      }
    }, "消息已提交，正在等待流式结果。");
    if (error.value) {
      streaming.value = false;
      clearPendingAssistant();
    }
  } finally {
    sending.value = false;
  }
}

async function cancelThread() {
  if (!activeSessionId.value || !activeThreadId.value) {
    return;
  }
  await runAction(async () => {
    streaming.value = false;
    closeThreadEvents();
    await callChatApi("cancelChatThread", activeSessionId.value, activeThreadId.value);
  }, "已请求取消当前线程。");
}

async function ensureActiveChatThread() {
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
  const payload = await callChatApi("createChatSessionWithThread", {
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

function applyChatSessionPayload(payload: ChatSessionPayload) {
  activeSessionId.value = payload.session_id;
  activeThreadId.value = payload.thread_id;
  activeBinding.value = {
    session_id: payload.session_id,
    thread_id: payload.thread_id,
    template_id: payload.template_id,
    effective_provider_id: payload.effective_provider_id,
    effective_model: payload.effective_model,
  };
  threadTitle.value = "新的对话线程";
  selectedTemplateId.value = payload.template_id;
  selectedProviderId.value = payload.effective_provider_id;
  selectedModel.value = payload.effective_model ?? selectedModel.value;
  if (!threads.value.some((thread) => thread.id === payload.thread_id)) {
    threads.value = [
      {
        id: payload.thread_id,
        title: null,
        turn_count: 0,
        token_count: 0,
        updated_at: new Date().toISOString(),
      },
      ...threads.value,
    ];
  }
}

function openThreadEvents(sessionId: string, threadId: string) {
  closeThreadEvents();
  if (!api.subscribeChatThread) {
    return;
  }

  try {
    streamSubscription.value = api.subscribeChatThread(sessionId, threadId, {
      onEvent: handleThreadEvent,
      onError: (reason) => {
        closeThreadEvents();
        if (streaming.value) {
          actionMessage.value = reason.message;
          void refreshStreamUntilSettled(countAssistantMessages());
        } else {
          setError(reason);
        }
      },
    });
  } catch (reason) {
    setError(reason);
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
      actionMessage.value = `正在重试第 ${payload.attempt}/${payload.max_retries} 次：${payload.error}`;
      break;
    case "tool_started":
      streaming.value = true;
      if (!pendingAssistantContent.value) {
        pendingAssistantContent.value = `正在调用工具：${payload.tool_name}`;
      }
      break;
    case "turn_failed":
      streaming.value = false;
      clearPendingAssistant();
      setError(payload.error);
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

async function settleThreadAfterStream() {
  await refreshActiveThread({ silent: true });
  if (countAssistantMessages() <= assistantCountAtStreamStart.value) {
    void refreshStreamUntilSettled(assistantCountAtStreamStart.value);
    return;
  }
  streaming.value = false;
  clearPendingAssistant();
  actionMessage.value = "回复已刷新。";
}

function applyPrompt(_event: MouseEvent, item: PromptProps) {
  if (item.id === "provider") {
    draftMessage.value = "请检查当前默认模型、提供方和智能体模板是否适合继续这个任务。";
    return;
  }
  if (item.id === "mcp") {
    draftMessage.value = "请帮我梳理当前 MCP 服务的运行风险、可用工具和下一步运维动作。";
    return;
  }

  draftMessage.value = "请基于当前智能体模板，给出系统提示词和工具配置的改进建议。";
}

async function runAction(action: () => Promise<void>, successMessage: string) {
  error.value = "";
  actionMessage.value = "";
  try {
    await action();
    actionMessage.value = successMessage;
  } catch (reason) {
    setError(reason);
  }
}

function setError(reason: unknown) {
  error.value = errorMessage(reason);
}

function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
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
      actionMessage.value = "回复已刷新。";
      return;
    }

    await waitForStreamTick();
  }

  streaming.value = false;
  clearPendingAssistant();
  actionMessage.value = "消息已提交，后端仍在处理；可点击刷新获取最新回复。";
}

function clearPendingAssistant() {
  pendingAssistantContent.value = "";
  pendingAssistantReasoning.value = "";
}

function waitForStreamTick() {
  if (import.meta.env.MODE === "test") {
    return Promise.resolve();
  }

  return new Promise<void>((resolve) => {
    window.setTimeout(resolve, 900);
  });
}

function countAssistantMessages() {
  return messages.value.filter((message) => message.role === "assistant").length;
}

function createLocalMessage(role: ChatMessageRecord["role"], content: string): ChatMessageRecord {
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

function selectDefaultTemplateAndProvider() {
  const firstProvider = providers.value.find((provider) => provider.is_default) ?? providers.value[0] ?? null;
  const firstTemplate = templates.value[0] ?? null;
  selectedProviderId.value = firstProvider?.id ?? null;
  selectedModel.value = firstProvider?.default_model ?? "";
  selectedTemplateId.value = firstTemplate?.id ?? null;
}

function emptyContentForRole(role: ChatMessageRecord["role"]) {
  if (role === "tool") {
    return "工具调用结果为空。";
  }

  return "消息内容为空。";
}

function displayMessageContent(message: ChatMessageRecord) {
  const content = message.content?.trim();
  if (content) {
    return message.content;
  }

  if (message.role === "assistant") {
    const names = toolCallNames(message.tool_calls);
    if (names.length > 0) {
      return `正在调用工具：${names.join("、")}`;
    }

    if (message.reasoning_content?.trim()) {
      return "助手正在思考，等待可见回复。";
    }
  }

  if (message.role === "tool" && message.name?.trim()) {
    return `工具 ${message.name} 返回为空。`;
  }

  return emptyContentForRole(message.role);
}

function toolCallNames(toolCalls: unknown[] | null | undefined) {
  if (!Array.isArray(toolCalls)) {
    return [];
  }

  return toolCalls
    .map((toolCall) => {
      if (!toolCall || typeof toolCall !== "object" || !("name" in toolCall)) {
        return "";
      }

      const name = (toolCall as { name?: unknown }).name;
      return typeof name === "string" ? name.trim() : "";
    })
    .filter((name) => name.length > 0);
}

function normalizedSessionName() {
  return sessionName.value.trim() || "新的 Web 对话";
}

function formatSessionName(session: ChatSessionSummary) {
  const name = session.name.trim();
  return name || `会话 ${session.id.slice(0, 8)}`;
}

function formatThreadTitle(thread: ChatThreadSummary) {
  return thread.title || `线程 ${thread.id.slice(0, 8)}`;
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

async function callChatApi<K extends keyof ChatApiMethods>(
  method: K,
  ...args: Parameters<ChatApiMethods[K]>
): Promise<Awaited<ReturnType<ChatApiMethods[K]>>> {
  const fn = api[method] as ChatApiMethods[K] | undefined;
  if (typeof fn !== "function") {
    throw new Error(`当前 API client 未实现 ${String(method)}。`);
  }

  const bound = fn.bind(api) as (...nextArgs: Parameters<ChatApiMethods[K]>) => ReturnType<ChatApiMethods[K]>;
  return (await bound(...args)) as Awaited<ReturnType<ChatApiMethods[K]>>;
}
</script>

<template>
  <section class="chat-page">
    <div class="chat-workspace">
      <aside class="chat-sidebar shell-card">
        <section class="sidebar-pane">
          <div class="panel-heading">
            <div>
              <p class="eyebrow">Session</p>
              <h3 class="section-heading">会话</h3>
            </div>
            <TinyTag type="info">{{ stats.sessions }} 个</TinyTag>
          </div>

          <div class="create-row">
            <TinyInput v-model="sessionName" name="session-name" placeholder="当前会话名称" />
            <TinyButton data-testid="create-session" type="primary" @click="startNewChatDraft">新对话</TinyButton>
          </div>

          <div class="conversation-config">
            <label>
              <span>智能体模板</span>
              <TinySelect v-model="selectedTemplateId" data-testid="conversation-template-select" name="conversation-template">
                <TinyOption v-for="item in templates" :key="item.id" :label="item.display_name" :value="item.id" />
              </TinySelect>
            </label>
            <p v-if="selectedTemplate" class="template-hint">
              {{ selectedTemplate.description || `模板版本 ${selectedTemplate.version}` }}
            </p>
            <p v-else class="template-hint template-hint--warning">暂无可用智能体模板，无法创建新对话。</p>
          </div>

          <div v-if="loading" class="empty-state">正在加载对话会话…</div>
          <div v-else-if="sessions.length === 0" class="empty-state">暂无对话会话，发送第一条消息后会自动创建。</div>
          <div v-else class="session-list">
            <button
              v-for="sessionItem in sessions"
              :key="sessionItem.id"
              class="session-item"
              :class="{ active: sessionItem.id === activeSessionId }"
              type="button"
              @click="selectSession(sessionItem.id)"
            >
              <span>{{ formatSessionName(sessionItem) }}</span>
              <small>{{ sessionItem.thread_count }} 线程 · {{ formatDate(sessionItem.updated_at) }}</small>
            </button>
          </div>

          <div v-if="activeSession" class="rail-actions">
            <TinyButton @click="renameSession">保存会话名</TinyButton>
            <TinyButton :disabled="deleting" @click="deleteSession">删除会话</TinyButton>
          </div>
        </section>

        <section class="sidebar-pane">
          <div class="panel-heading">
            <div>
              <p class="eyebrow">Thread</p>
              <h3 class="section-heading">线程</h3>
            </div>
            <TinyTag type="info">{{ stats.threads }} 个</TinyTag>
          </div>

          <div class="thread-config">
            <label>
              <span>模型提供方</span>
              <TinySelect v-model="selectedProviderId" name="provider">
                <TinyOption v-for="item in providers" :key="item.id" :label="item.display_name" :value="item.id" />
              </TinySelect>
            </label>
            <label>
              <span>模型</span>
              <TinyInput v-model="selectedModel" name="model" placeholder="例如 glm-4.7" />
            </label>
            <TinyButton data-testid="create-thread" :disabled="!canCreateThread || creatingThread" type="primary" @click="createThread">
              {{ creatingThread ? "创建中" : "创建线程" }}
            </TinyButton>
          </div>

          <div v-if="!activeSession" class="empty-state">当前是草稿对话，发送第一条消息后会创建线程。</div>
          <div v-else-if="threads.length === 0" class="empty-state">当前会话还没有线程。</div>
          <div v-else class="thread-list">
            <button
              v-for="threadItem in threads"
              :key="threadItem.id"
              class="thread-item"
              :class="{ active: threadItem.id === activeThreadId }"
              type="button"
              @click="selectThread(threadItem.id)"
            >
              <span>{{ formatThreadTitle(threadItem) }}</span>
              <small>{{ threadItem.turn_count }} turn · {{ threadItem.token_count }} token</small>
            </button>
          </div>

          <div v-if="hasActiveThread" class="thread-editor">
            <TinyInput v-model="threadTitle" name="thread-title" placeholder="线程标题" />
            <TinyButton @click="renameThread">保存标题</TinyButton>
            <TinyButton @click="applyModelBinding">应用模型</TinyButton>
            <TinyButton :disabled="deleting" @click="deleteThread">删除线程</TinyButton>
          </div>
        </section>
      </aside>

      <div class="chat-main-column">
        <article class="chat-panel shell-card">
          <header class="chat-panel__header">
            <div>
              <p class="eyebrow">Conversation</p>
              <h3 class="section-heading">{{ currentConversationTitle }}</h3>
              <p class="section-copy">
                {{ activeBinding?.effective_model || selectedModel || "未绑定模型" }}
                <span v-if="activeProvider"> · {{ activeProvider.display_name }}</span>
              </p>
            </div>
            <div class="chat-actions">
              <TinyButton :disabled="!hasActiveThread" @click="refreshActiveThread">刷新</TinyButton>
              <TinyButton :disabled="!hasActiveThread" @click="activateThread">激活</TinyButton>
              <TinyButton data-testid="cancel-thread" :disabled="!hasActiveThread" @click="cancelThread">取消运行</TinyButton>
            </div>
          </header>

          <div v-if="error" class="notice notice--danger">{{ error }}</div>
          <div v-if="actionMessage" class="notice notice--success">{{ actionMessage }}</div>

          <div class="message-stage">
            <div v-if="threadLoading && robotMessages.length === 0" class="empty-state">正在刷新消息…</div>
            <TrBubbleList
              v-else-if="robotMessages.length > 0"
              class="bubble-list"
              :messages="robotMessages"
              :role-configs="bubbleRoles"
              auto-scroll
              group-strategy="divider"
            />
            <div v-else class="prompt-panel">
              <p class="prompt-title">快速开始</p>
              <TrPrompts :items="starterPrompts" wrap @item-click="applyPrompt" />
            </div>
          </div>
        </article>

        <footer class="composer-panel shell-card">
          <TrSender
            v-model="draftMessage"
            class="chat-sender"
            :clearable="true"
            :disabled="!canSendMessage"
            :loading="sending"
            :placeholder="senderPlaceholder"
            stop-text="取消运行"
            @submit="sendMessage"
            @cancel="cancelThread"
          />
        </footer>
      </div>
    </div>
  </section>
</template>

<style scoped>
.chat-page {
  width: 100%;
  min-height: 760px;
}

.chat-workspace {
  display: grid;
  grid-template-columns: minmax(260px, 320px) minmax(0, 1fr);
  gap: var(--space-5);
  min-height: 760px;
}

.chat-main-column {
  display: grid;
  grid-template-rows: minmax(0, 1fr) auto;
  gap: var(--space-5);
  min-height: 0;
}

.chat-panel,
.chat-sidebar,
.composer-panel {
  padding: var(--space-5);
}

.chat-sidebar {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
  min-height: 0;
  max-height: 760px;
  overflow: auto;
}

.sidebar-pane {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.composer-panel {
  position: sticky;
  bottom: var(--space-4);
  z-index: 2;
}

.chat-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  min-height: 0;
  --tr-bubble-list-padding: 0;
  --tr-bubble-list-gap: var(--space-4);
  --tr-bubble-box-bg: var(--surface-overlay);
  --tr-bubble-box-border: 1px solid var(--border-default);
  --tr-bubble-box-border-radius: var(--radius-lg);
  --tr-bubble-text-color: var(--text-primary);
  --tr-bubble-text-font-size: var(--text-sm);
  --tr-sender-bg-color: var(--surface-base);
  --tr-sender-text-color: var(--text-primary);
  --tr-sender-placeholder-color: var(--text-placeholder);
  --tr-prompt-bg: var(--surface-base);
  --tr-prompt-bg-hover: var(--accent-subtle);
  --tr-prompt-border-radius: var(--radius-lg);
  --tr-prompt-shadow: none;
  --tr-prompt-title-color: var(--text-primary);
  --tr-prompt-description-color: var(--text-muted);
}

.panel-heading,
.chat-panel__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
}

.create-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: var(--space-2);
}

.session-list,
.thread-list {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: var(--space-2);
  min-height: 0;
  overflow: auto;
}

.session-item,
.thread-item {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  width: 100%;
  padding: var(--space-3);
  background: transparent;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  text-align: left;
  transition:
    background var(--transition-base),
    border-color var(--transition-base),
    color var(--transition-base);
}

.session-item span,
.thread-item span {
  color: var(--text-primary);
  font-weight: 590;
}

.session-item small,
.thread-item small {
  color: var(--text-muted);
  font-size: var(--text-xs);
}

.session-item:hover,
.thread-item:hover,
.session-item.active,
.thread-item.active {
  background: var(--accent-subtle);
  border-color: var(--accent);
}

.rail-actions,
.chat-actions,
.thread-editor {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.conversation-config,
.thread-config {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.conversation-config label,
.thread-config label {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 590;
}

.template-hint {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--text-xs);
  line-height: 1.5;
}

.template-hint--warning {
  color: var(--warning);
}

.notice {
  padding: var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  line-height: 1.5;
}

.notice--danger {
  background: var(--danger-bg);
  border: 1px solid var(--danger-border);
  color: var(--danger);
}

.notice--success {
  background: var(--success-bg);
  border: 1px solid var(--success-border);
  color: var(--success);
}

.message-stage {
  flex: 1;
  min-height: 500px;
  max-height: 650px;
  overflow: auto;
  padding: var(--space-4);
  background:
    radial-gradient(circle at top left, var(--accent-subtle), transparent 34%),
    var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
}

.bubble-list {
  min-height: 100%;
}

.prompt-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.prompt-title {
  margin: 0;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-weight: 590;
}

.empty-state {
  display: grid;
  min-height: 128px;
  place-items: center;
  color: var(--text-muted);
  font-size: var(--text-sm);
  line-height: 1.6;
  text-align: center;
}

.chat-sender {
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
}

:deep([data-role="user"]) {
  --tr-bubble-box-bg: var(--accent-subtle);
  --tr-bubble-box-border: 1px solid rgba(94, 106, 210, 0.2);
}

.chat-avatar,
.prompt-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  height: 28px;
  padding: 0 var(--space-1);
  border-radius: var(--radius-full);
  background: var(--accent-subtle);
  color: var(--accent);
  font-size: var(--text-xs);
  font-weight: 700;
}

.chat-avatar--user {
  background: var(--success-bg);
  color: var(--success);
}

.chat-avatar--tool {
  background: var(--warning-bg);
  color: var(--warning);
}

@media (max-width: 1180px) {
  .chat-workspace {
    grid-template-columns: minmax(0, 1fr);
  }

  .chat-main-column {
    min-height: 0;
  }

  .chat-sidebar {
    max-height: none;
  }

  .message-stage {
    min-height: 420px;
    max-height: none;
  }
}
</style>
