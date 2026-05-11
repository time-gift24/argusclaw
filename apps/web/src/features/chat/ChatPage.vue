<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import type { PromptProps } from "@opentiny/tiny-robot";

import {
  getApiClient,
  type AgentRecord,
  type ChatSessionSummary,
  type ChatThreadBinding,
  type ChatThreadJobSummary,
  type ChatThreadSummary,
  type LlmProviderRecord,
} from "@/lib/api";
import { useChatSessions } from "./composables/useChatSessions";
import { useChatThreadStream } from "./composables/useChatThreadStream";
import { useChatComposer } from "./composables/useChatComposer";
import {
  createBubbleRoles,
  createStarterPrompts,
  draftMessageForPrompt,
  toRobotMessages,
} from "./composables/useChatPresentation";
import ChatComposerBar from "./components/ChatComposerBar.vue";
import ChatConversationPanel from "./components/ChatConversationPanel.vue";
import ChatHistoryDialog from "./components/ChatHistoryDialog.vue";
import DispatchedJobsPanel from "./components/DispatchedJobsPanel.vue";
import RuntimeActivityRail from "./components/RuntimeActivityRail.vue";

const router = useRouter();
const route = useRoute();
const chatSessions = useChatSessions();
const chatThreadStream = useChatThreadStream({
  activeSessionId: chatSessions.activeSessionId,
  activeThreadId: chatSessions.activeThreadId,
});

const providers = ref<LlmProviderRecord[]>([]);
const templates = ref<AgentRecord[]>([]);
const selectedTemplateId = ref<number | null>(null);
const selectedProviderId = ref<number | null>(null);
const selectedModel = ref("");
const dispatchedJobs = ref<ChatThreadJobSummary[]>([]);
const dispatchedJobsLoading = ref(false);
const dispatchedJobsError = ref("");
const chatBodyStreamRef = ref<HTMLElement | null>(null);
const shouldStickToBottom = ref(true);
const AUTO_SCROLL_THRESHOLD = 72;
const DISPATCHED_JOBS_REFRESH_DELAY = 180;
let dispatchedJobsRequestId = 0;
let dispatchedJobsRefreshTimer: number | null = null;

const chatComposer = useChatComposer({
  activeSessionId: chatSessions.activeSessionId,
  activeThreadId: chatSessions.activeThreadId,
  activeBinding: chatSessions.activeBinding,
  selectedTemplateId,
  selectedProviderId,
  selectedModel,
  providers,
  templates,
  sessionName: chatSessions.sessionName,
  threadTitle: chatSessions.threadTitle,
  threads: chatSessions.threads,
  refreshSessions: chatSessions.refreshSessions,
  refreshThreads: chatSessions.refreshThreads,
  applyChatSessionPayload: chatSessions.applyChatSessionPayload,
  openThreadEvents: chatThreadStream.openThreadEvents,
  closeThreadEvents: chatThreadStream.closeThreadEvents,
  resetRuntimeActivity: chatThreadStream.resetRuntimeActivity,
  refreshStreamUntilSettled: chatThreadStream.refreshStreamUntilSettled,
  countAssistantMessages: chatThreadStream.countAssistantMessages,
  clearPendingAssistant: chatThreadStream.clearPendingAssistant,
  streaming: chatThreadStream.streaming,
  assistantCountAtStreamStart: chatThreadStream.assistantCountAtStreamStart,
  messages: chatThreadStream.messages,
});

const historyDialogOpen = ref(false);

const hasActiveThread = computed(() => Boolean(chatSessions.activeSessionId.value && chatSessions.activeThreadId.value));
const activeProvider = computed(() => providers.value.find((p) => p.id === Number(selectedProviderId.value)) ?? null);
const selectedTemplate = computed(() => templates.value.find((t) => t.id === Number(selectedTemplateId.value)) ?? null);
const defaultProvider = computed(() => providers.value.find((p) => p.is_default) ?? providers.value[0] ?? null);

const robotMessages = computed(() => toRobotMessages({
  messages: chatThreadStream.messages.value,
  streaming: chatThreadStream.streaming.value,
  hasActiveThread: hasActiveThread.value,
  pendingAssistantContent: chatThreadStream.pendingAssistantContent.value,
  pendingAssistantReasoning: chatThreadStream.pendingAssistantReasoning.value,
  runtimeActivities: [],
}));
const bubbleRoles = createBubbleRoles();
const starterPrompts = createStarterPrompts();
const showDispatchedJobsPanel = computed(
  () => dispatchedJobsLoading.value || Boolean(dispatchedJobsError.value) || dispatchedJobs.value.length > 0,
);

function getDistanceFromBottom(element: HTMLElement) {
  return element.scrollHeight - element.clientHeight - element.scrollTop;
}

function updateStickToBottom() {
  const stream = chatBodyStreamRef.value;
  if (!stream) return;
  shouldStickToBottom.value = getDistanceFromBottom(stream) <= AUTO_SCROLL_THRESHOLD;
}

function scrollChatBodyToBottom() {
  const stream = chatBodyStreamRef.value;
  if (!stream) return;

  const top = stream.scrollHeight;
  if (typeof stream.scrollTo === "function") {
    stream.scrollTo({ top, behavior: "auto" });
  } else {
    stream.scrollTop = top;
  }
}

function handleChatBodyScroll() {
  updateStickToBottom();
}

onMounted(() => {
  void loadInitialState();
});

onBeforeUnmount(() => {
  chatThreadStream.closeThreadEvents();
  if (dispatchedJobsRefreshTimer !== null) {
    window.clearTimeout(dispatchedJobsRefreshTimer);
  }
});

async function loadInitialState() {
  const api = getApiClient();
  const preferredSessionId = getStringQueryValue(route.query.session);
  const preferredThreadId = getStringQueryValue(route.query.thread);
  const loadErrors: string[] = [];
  chatSessions.loading.value = true;
  chatComposer.error.value = "";
  try {
    if (api.getChatOptions) {
      try {
        const options = await api.getChatOptions();
        providers.value = options.providers;
        templates.value = options.templates;
      } catch (reason) {
        loadErrors.push(`对话配置加载失败：${formatErrorMessage(reason)}`);
      }
    } else {
      const [providersResult, templatesResult] = await Promise.allSettled([
        api.listProviders(),
        api.listTemplates(),
      ]);
      if (providersResult.status === "fulfilled") providers.value = providersResult.value;
      else loadErrors.push(`模型提供方加载失败：${formatErrorMessage(providersResult.reason)}`);
      if (templatesResult.status === "fulfilled") templates.value = templatesResult.value;
      else loadErrors.push(`智能体模板加载失败：${formatErrorMessage(templatesResult.reason)}`);
    }

    const firstTemplate = templates.value[0] ?? null;
    if (firstTemplate) {
      applyTemplateSelection(firstTemplate.id);
    } else {
      selectedProviderId.value = defaultProvider.value?.id ?? null;
      selectedModel.value = defaultProvider.value?.default_model ?? "";
      selectedTemplateId.value = null;
    }

    try {
      await chatSessions.loadInitialState(preferredSessionId, preferredThreadId);
      if (chatSessions.activeSessionId.value && chatSessions.activeThreadId.value) {
        try {
          await syncActiveThreadBinding();
        } catch (reason) {
          loadErrors.push(`对话线程激活失败：${formatErrorMessage(reason)}`);
        }
        await chatThreadStream.refreshActiveThread({ silent: true });
      }
    } catch (reason) {
      loadErrors.push(`对话会话加载失败：${formatErrorMessage(reason)}`);
    }

    if (loadErrors.length > 0) {
      chatComposer.error.value = loadErrors.join("；");
    }
  } finally {
    chatSessions.loading.value = false;
  }
}

function getStringQueryValue(value: unknown) {
  return typeof value === "string" ? value : undefined;
}

function formatErrorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

function clearDispatchedJobs() {
  dispatchedJobsRequestId += 1;
  dispatchedJobs.value = [];
  dispatchedJobsLoading.value = false;
  dispatchedJobsError.value = "";
}

async function refreshDispatchedJobs() {
  const sessionId = chatSessions.activeSessionId.value;
  const threadId = chatSessions.activeThreadId.value;
  if (!sessionId || !threadId) {
    clearDispatchedJobs();
    return;
  }

  const requestId = ++dispatchedJobsRequestId;
  dispatchedJobsLoading.value = true;
  dispatchedJobsError.value = "";
  try {
    const api = getApiClient();
    if (!api.listChatThreadJobs) {
      dispatchedJobs.value = [];
      dispatchedJobsError.value = "当前后端不支持派发记录接口";
      return;
    }

    const jobs = await api.listChatThreadJobs(sessionId, threadId);
    if (requestId !== dispatchedJobsRequestId) return;
    dispatchedJobs.value = jobs;
  } catch (reason) {
    if (requestId !== dispatchedJobsRequestId) return;
    dispatchedJobs.value = [];
    dispatchedJobsError.value = formatErrorMessage(reason);
  } finally {
    if (requestId === dispatchedJobsRequestId) {
      dispatchedJobsLoading.value = false;
    }
  }
}

function scheduleDispatchedJobsRefresh() {
  if (dispatchedJobsRefreshTimer !== null) {
    window.clearTimeout(dispatchedJobsRefreshTimer);
  }
  dispatchedJobsRefreshTimer = window.setTimeout(() => {
    dispatchedJobsRefreshTimer = null;
    void refreshDispatchedJobs();
  }, DISPATCHED_JOBS_REFRESH_DELAY);
}

function isTemporaryMissingChatJobRoute(reason: unknown) {
  return reason instanceof Error && reason.message.includes("No match") && reason.message.includes("chat-job");
}

function handleOpenJobFailure(reason: unknown) {
  if (isTemporaryMissingChatJobRoute(reason)) return;
  console.error("Failed to open dispatched job", reason);
}

function openJob(jobId: string) {
  let navigation: ReturnType<typeof router.push>;
  try {
    navigation = router.push({
      name: "chat-job",
      params: { jobId },
      query: {
        fromSession: chatSessions.activeSessionId.value,
        fromThread: chatSessions.activeThreadId.value,
      },
    });
  } catch (reason) {
    handleOpenJobFailure(reason);
    return;
  }

  void navigation.catch(handleOpenJobFailure);
}

watch(
  () => chatSessions.activeThreadId.value,
  (threadId, previousThreadId) => {
    const preservingPendingFirstTurn =
      !previousThreadId && Boolean(threadId) && chatComposer.sending.value;

    if (!preservingPendingFirstTurn) {
      chatThreadStream.resetTransientState();
    }
    if (threadId && chatSessions.activeSessionId.value) {
      chatThreadStream.resetRuntimeActivity();
      chatThreadStream.openThreadEvents(chatSessions.activeSessionId.value, threadId);
    }
  },
);

watch(
  () => [chatSessions.activeSessionId.value, chatSessions.activeThreadId.value] as const,
  ([sessionId, threadId]) => {
    if (!sessionId || !threadId) {
      clearDispatchedJobs();
      return;
    }
    void refreshDispatchedJobs();
  },
);

watch(
  () =>
    chatThreadStream.runtimeActivities.value
      .filter((activity) => activity.kind === "job")
      .map((activity) => `${activity.id}:${activity.status}:${activity.resultPreview}`)
      .join("|"),
  (signature) => {
    if (!signature) return;
    scheduleDispatchedJobsRefresh();
  },
);

watch(
  () => robotMessages.value,
  async (messages) => {
    if (messages.length === 0 || !shouldStickToBottom.value) return;
    await nextTick();
    scrollChatBodyToBottom();
  },
  { deep: true },
);

async function handleSelectThreadFromDialog(sessionId: string, threadId: string) {
  chatThreadStream.closeThreadEvents();
  await chatSessions.selectSession(sessionId, threadId);
  await syncActiveThreadBinding();
  await chatThreadStream.refreshActiveThread();
}

async function handleRenameSession(sessionId: string, name: string) {
  const api = getApiClient();
  try {
    const renamed = await api.renameChatSession!(sessionId, name);
    chatSessions.sessions.value = chatSessions.sessions.value.map((s: ChatSessionSummary) =>
      s.id === renamed.id ? renamed : s,
    );
  } catch (reason) {
    chatComposer.error.value = reason instanceof Error ? reason.message : String(reason);
  }
}

async function handleDeleteSession(sessionId: string) {
  const api = getApiClient();
  try {
    chatThreadStream.closeThreadEvents();
    chatThreadStream.resetTransientState();
    chatThreadStream.resetRuntimeActivity();
    await api.deleteChatSession!(sessionId);
    await chatSessions.refreshSessions();
    chatSessions.activeSessionId.value = "";
    chatSessions.activeThreadId.value = "";
    chatSessions.threads.value = [];
    if (chatSessions.sessions.value.length > 0) {
      await chatSessions.selectSession(chatSessions.sessions.value[0].id);
      await syncActiveThreadBinding();
      await chatThreadStream.refreshActiveThread();
    }
  } catch (reason) {
    chatComposer.error.value = reason instanceof Error ? reason.message : String(reason);
  }
}

function handleNewChat() {
  chatThreadStream.closeThreadEvents();
  chatThreadStream.resetTransientState();
  chatSessions.startNewChatDraft();
  chatComposer.draftMessage.value = "";
  chatThreadStream.messages.value = [];
  chatThreadStream.resetRuntimeActivity();
  chatComposer.actionMessage.value = "";
}

async function handleTemplateChange(value: number | null) {
  const previousSelection = {
    templateId: selectedTemplateId.value,
    providerId: selectedProviderId.value,
    model: selectedModel.value,
    sessionId: chatSessions.activeSessionId.value,
    threadId: chatSessions.activeThreadId.value,
    binding: chatSessions.activeBinding.value,
    messages: chatThreadStream.messages.value,
  };
  const selection = applyTemplateSelection(value);
  chatComposer.error.value = "";
  chatComposer.actionMessage.value = "";

  if (!selection.template || !hasActiveConversationMessages()) {
    return;
  }

  try {
    const api = getApiClient();
    const payload = await api.createChatSessionWithThread!({
      name: "新的 Web 对话",
      template_id: selection.template.id,
      provider_id: selection.providerId,
      model: selection.model || null,
    });
    chatThreadStream.closeThreadEvents();
    chatThreadStream.resetTransientState();
    chatThreadStream.resetRuntimeActivity();
    chatThreadStream.messages.value = [];
    chatSessions.applyChatSessionPayload(payload);
    chatSessions.sessionName.value = "新的 Web 对话";
    await chatSessions.refreshSessions();
    await chatSessions.refreshThreads(payload.session_id);
    chatComposer.actionMessage.value = `已切换到「${selection.template.display_name}」，新的消息将在新会话中发送。`;
  } catch (reason) {
    selectedTemplateId.value = previousSelection.templateId;
    selectedProviderId.value = previousSelection.providerId;
    selectedModel.value = previousSelection.model;
    chatSessions.activeSessionId.value = previousSelection.sessionId;
    chatSessions.activeThreadId.value = previousSelection.threadId;
    chatSessions.activeBinding.value = previousSelection.binding;
    chatThreadStream.messages.value = previousSelection.messages;
    chatComposer.error.value = formatErrorMessage(reason);
    chatComposer.actionMessage.value = "";
  }
}

function applyTemplateSelection(templateId: number | null) {
  selectedTemplateId.value = templateId;
  const template = templates.value.find((candidate) => candidate.id === Number(templateId)) ?? null;
  const templateProviderId = template?.provider_id ?? null;
  const provider =
    providers.value.find((candidate) => templateProviderId !== null && candidate.id === templateProviderId) ??
    providers.value.find((candidate) => candidate.id === selectedProviderId.value) ??
    defaultProvider.value;
  selectedProviderId.value = provider?.id ?? null;
  selectedModel.value = template?.model_id || provider?.default_model || "";
  return {
    template,
    providerId: provider?.id ?? null,
    model: selectedModel.value,
  };
}

async function syncActiveThreadBinding() {
  if (!chatSessions.activeSessionId.value || !chatSessions.activeThreadId.value) return;
  await chatSessions.activateThread();
  applyThreadBindingSelection(chatSessions.activeBinding.value);
}

function applyThreadBindingSelection(binding: ChatThreadBinding | null) {
  if (!binding) return;

  selectedTemplateId.value = binding.template_id;
  const template = templates.value.find((candidate) => candidate.id === binding.template_id) ?? null;
  const templateProviderId = template?.provider_id ?? null;
  const provider =
    providers.value.find(
      (candidate) => binding.effective_provider_id !== null && candidate.id === binding.effective_provider_id,
    ) ??
    providers.value.find((candidate) => templateProviderId !== null && candidate.id === templateProviderId) ??
    defaultProvider.value;

  selectedProviderId.value = provider?.id ?? null;
  selectedModel.value = binding.effective_model || template?.model_id || provider?.default_model || "";
}

function hasActiveConversationMessages() {
  return Boolean(
    chatSessions.activeSessionId.value &&
      chatSessions.activeThreadId.value &&
      chatThreadStream.messages.value.length > 0,
  );
}

function applyPrompt(_event: MouseEvent, item: PromptProps) {
  chatComposer.draftMessage.value = draftMessageForPrompt(item.id);
}
</script>

<template>
  <section
    ref="chatBodyStreamRef"
    class="chat-page chat-page--immersive chat-page--single-scroll"
    :class="{ 'chat-page--with-dispatched-jobs': showDispatchedJobsPanel }"
    @scroll.passive="handleChatBodyScroll"
  >
    <div class="chat-runtime-floating-layer">
      <RuntimeActivityRail :activities="chatThreadStream.runtimeActivities.value" />
      <DispatchedJobsPanel
        v-if="showDispatchedJobsPanel"
        :jobs="dispatchedJobs"
        :loading="dispatchedJobsLoading"
        :error="dispatchedJobsError"
        @open-job="openJob"
        @refresh="refreshDispatchedJobs"
      />
    </div>

    <div class="chat-body-stream">
      <ChatConversationPanel
        error=""
        :notice="chatComposer.actionMessage.value"
        :thread-loading="chatThreadStream.threadLoading.value"
        :robot-messages="robotMessages"
        :bubble-roles="bubbleRoles"
        :starter-prompts="starterPrompts"
        @prompt="applyPrompt"
      />
    </div>

    <div class="chat-page__composer-dock">
      <div class="chat-page__composer-shell">
        <div
          v-if="chatComposer.error.value"
          class="chat-page__composer-error error-message"
          role="alert"
        >
          {{ chatComposer.error.value }}
        </div>
        <ChatComposerBar
          v-model="chatComposer.draftMessage.value"
          :templates="templates"
          :providers="providers"
          :selected-template-id="selectedTemplateId"
          v-model:selected-provider-id="selectedProviderId"
          v-model:selected-model="selectedModel"
          :disabled="!chatComposer.canSendMessage.value"
          :loading="chatComposer.sending.value || chatThreadStream.streaming.value"
          :placeholder="chatComposer.senderPlaceholder.value"
          :has-active-thread="hasActiveThread"
          :active-provider="activeProvider"
          :selected-template="selectedTemplate"
          @submit="chatComposer.sendMessage"
          @cancel="chatComposer.cancelThread"
          @new-chat="handleNewChat"
          @open-history="historyDialogOpen = true"
          @update:selected-template-id="handleTemplateChange"
        />
      </div>
    </div>

    <ChatHistoryDialog
      v-model="historyDialogOpen"
      :sessions="chatSessions.sessions.value"
      :active-session-id="chatSessions.activeSessionId.value"
      :active-thread-id="chatSessions.activeThreadId.value"
      :session-list-loading="chatSessions.loading.value"
      @select-thread="handleSelectThreadFromDialog"
      @delete-session="handleDeleteSession"
      @rename-session="handleRenameSession"
    />
  </section>
</template>

<style scoped>
.chat-page {
  --chat-composer-width: 1120px;
  --chat-rail-width: 320px;
  --chat-layout-gap: var(--space-5);
  --chat-dock-clearance: 132px;
  height: 100vh;
  min-height: 100vh;
  max-height: 100vh;
  padding: 0;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.chat-page--immersive {
  position: relative;
}

.chat-page.chat-page--immersive {
  width: calc(100% + var(--space-6) + var(--space-6));
  margin: calc(0px - var(--space-6));
}

.chat-page--single-scroll {
  scroll-behavior: smooth;
}

.chat-body-stream {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: auto;
  min-width: 0;
  min-height: 100%;
  overflow-x: hidden;
  overflow-y: visible;
  padding: var(--space-6) var(--space-6) 0;
  overscroll-behavior: contain;
}

.chat-body-stream::after {
  content: "";
  flex: 0 0 calc(var(--chat-dock-clearance, 132px) + var(--space-6));
}

.chat-runtime-floating-layer {
  position: absolute;
  top: var(--space-6);
  right: var(--space-6);
  z-index: 25;
  display: grid;
  gap: var(--space-3);
  width: var(--chat-rail-width);
  max-width: calc(100vw - 260px - var(--space-6) - var(--space-6));
  pointer-events: none;
}

.chat-runtime-floating-layer :deep(.runtime-rail),
.chat-runtime-floating-layer :deep(.dispatched-jobs) {
  width: 100%;
  pointer-events: auto;
}

.chat-runtime-floating-layer :deep(.runtime-rail--collapsed) {
  width: min(100%, 188px);
  margin-left: auto;
}

.chat-page__composer-dock {
  position: sticky;
  left: var(--space-6);
  right: var(--space-6);
  bottom: var(--space-6);
  z-index: 30;
  display: flex;
  justify-content: center;
}

.chat-page__composer-shell {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  width: min(100%, var(--chat-composer-width));
}

.chat-page__composer-error {
  margin: 0;
  padding: var(--space-3);
  border: 1px solid var(--danger-border);
  border-radius: var(--radius-md);
  background: var(--danger-bg);
  color: var(--danger);
  font-size: var(--text-sm);
}

@media (min-width: 1281px) {
  .chat-page--with-dispatched-jobs .chat-body-stream {
    padding-right: calc(var(--chat-rail-width) + var(--chat-layout-gap) + var(--space-6));
  }

  .chat-page--with-dispatched-jobs .chat-page__composer-dock {
    padding-right: calc(var(--chat-rail-width) + var(--chat-layout-gap));
  }
}

@media (max-width: 1180px) {
  .chat-page {
    --chat-dock-clearance: 160px;
  }

  .chat-page.chat-page--immersive {
    width: calc(100% + var(--space-4) + var(--space-4));
    margin: calc(0px - var(--space-4));
  }

  .chat-body-stream {
    padding: var(--space-4) var(--space-4) 0;
  }

  .chat-page__composer-dock {
    left: var(--space-4);
    right: var(--space-4);
    bottom: var(--space-4);
  }
}

@media (max-width: 1280px) {
  .chat-page--with-dispatched-jobs .chat-runtime-floating-layer {
    position: static;
    width: 100%;
    max-width: none;
    padding: var(--space-4) var(--space-4) 0;
    pointer-events: none;
  }

  .chat-runtime-floating-layer :deep(.runtime-rail--collapsed) {
    width: min(100%, 188px);
  }

  .chat-page__composer-shell {
    width: min(100%, var(--chat-composer-width));
  }
}
</style>
