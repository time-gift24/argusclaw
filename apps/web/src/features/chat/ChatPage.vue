<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { PromptProps } from "@opentiny/tiny-robot";

import {
  getApiClient,
  type AgentRecord,
  type ChatSessionSummary,
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
import RuntimeActivityRail from "./components/RuntimeActivityRail.vue";

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

onMounted(() => {
  void loadInitialState();
});

onBeforeUnmount(() => {
  chatThreadStream.closeThreadEvents();
});

async function loadInitialState() {
  const api = getApiClient();
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
      await chatSessions.loadInitialState();
      if (chatSessions.activeSessionId.value && chatSessions.activeThreadId.value) {
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

function formatErrorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
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

async function handleSelectThreadFromDialog(sessionId: string, threadId: string) {
  chatThreadStream.closeThreadEvents();
  await chatSessions.selectSession(sessionId, threadId);
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
  <section class="chat-page chat-page--immersive chat-page--single-scroll">
    <div class="chat-workspace">
      <div class="chat-main-column">
        <ChatConversationPanel
          :error="chatComposer.error.value"
          :notice="chatComposer.actionMessage.value"
          :thread-loading="chatThreadStream.threadLoading.value"
          :robot-messages="robotMessages"
          :bubble-roles="bubbleRoles"
          :starter-prompts="starterPrompts"
          @prompt="applyPrompt"
        />
      </div>
      <RuntimeActivityRail :activities="chatThreadStream.runtimeActivities.value" />
    </div>

    <div class="chat-page__composer-dock">
      <div class="chat-page__composer-shell">
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
  width: 100%;
  --chat-main-width: 1280px;
  --chat-rail-width: 320px;
  --chat-layout-gap: var(--space-5);
  --chat-dock-clearance: 212px;
  height: calc(100vh - (var(--space-6) * 2));
  min-height: calc(100vh - (var(--space-6) * 2));
  max-height: calc(100vh - (var(--space-6) * 2));
}

.chat-page--immersive {
  position: relative;
}

.chat-page--single-scroll {
  overflow: hidden;
}

.chat-workspace,
.chat-main-column {
  height: 100%;
  min-height: 0;
}

.chat-workspace {
  display: grid;
  grid-template-columns: minmax(0, 1fr) var(--chat-rail-width);
  gap: var(--chat-layout-gap);
  width: min(100%, var(--chat-main-width));
  margin-inline: auto;
}

.chat-main-column {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.chat-page__composer-dock {
  position: fixed;
  left: calc(260px + var(--space-6));
  right: var(--space-6);
  bottom: var(--space-6);
  z-index: 30;
  display: flex;
  justify-content: center;
}

.chat-page__composer-shell {
  width: min(100%, calc(var(--chat-main-width) - var(--chat-rail-width) - var(--chat-layout-gap)));
}

@media (max-width: 1180px) {
  .chat-page {
    --chat-dock-clearance: 228px;
    height: calc(100vh - (var(--space-4) * 2));
    min-height: calc(100vh - (var(--space-4) * 2));
    max-height: calc(100vh - (var(--space-4) * 2));
  }

  .chat-page__composer-dock {
    left: var(--space-4);
    right: var(--space-4);
    bottom: var(--space-4);
  }
}

@media (max-width: 1280px) {
  .chat-workspace {
    grid-template-columns: 1fr;
    overflow: auto;
    padding-bottom: calc(var(--chat-dock-clearance, 228px) + var(--space-4));
  }

  .chat-page__composer-shell {
    width: min(100%, var(--chat-main-width));
  }
}
</style>
