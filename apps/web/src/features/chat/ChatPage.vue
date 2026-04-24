<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { TrBubbleList, TrPrompts, type BubbleRoleConfig, type PromptProps } from "@opentiny/tiny-robot";

import { getApiClient, type AgentRecord, type ChatMessageRecord, type ChatSessionSummary, type ChatThreadSummary, type LlmProviderRecord } from "@/lib/api";
import { TinyButton, TinyTag } from "@/lib/opentiny";
import { useChatSessions } from "./composables/useChatSessions";
import { useChatThreadStream } from "./composables/useChatThreadStream";
import { useChatComposer } from "./composables/useChatComposer";
import ChatComposerBar from "./components/ChatComposerBar.vue";
import ChatHistoryDialog from "./components/ChatHistoryDialog.vue";

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
  messages: chatThreadStream.messages,
});

const historyDialogOpen = ref(false);

const hasActiveThread = computed(() => Boolean(chatSessions.activeSessionId.value && chatSessions.activeThreadId.value));
const activeProvider = computed(() => providers.value.find((p) => p.id === Number(selectedProviderId.value)) ?? null);
const selectedTemplate = computed(() => templates.value.find((t) => t.id === Number(selectedTemplateId.value)) ?? null);

const currentConversationTitle = computed(() => {
  if (chatSessions.activeThread.value) {
    return chatSessions.activeThread.value.title || `线程 ${chatSessions.activeThread.value.id.slice(0, 8)}`;
  }
  return hasActiveThread.value ? "新的对话线程" : "新对话草稿";
});

const robotMessages = computed(() => {
  const msgs = chatThreadStream.messages.value
    .filter((m) => m.role !== "system")
    .map((m) => ({
      role: m.role,
      content: displayMessageContent(m),
    }));

  if (chatThreadStream.streaming.value && (hasActiveThread.value || msgs.length > 0)) {
    msgs.push({
      role: "assistant",
      content: chatThreadStream.pendingAssistantContent.value ||
        (chatThreadStream.pendingAssistantReasoning.value ? "正在思考…" : "正在生成回复…"),
    });
  }
  return msgs;
});

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

onMounted(() => {
  void loadInitialState();
});

onBeforeUnmount(() => {
  chatThreadStream.closeThreadEvents();
});

async function loadInitialState() {
  const api = getApiClient();
  chatSessions.loading.value = true;
  try {
    const [providersResult, templatesResult] = await Promise.allSettled([
      api.listProviders(),
      api.listTemplates(),
    ]);
    if (providersResult.status === "fulfilled") providers.value = providersResult.value;
    if (templatesResult.status === "fulfilled") templates.value = templatesResult.value;

    const firstProvider = providers.value.find((p) => p.is_default) ?? providers.value[0] ?? null;
    const firstTemplate = templates.value[0] ?? null;
    selectedProviderId.value = firstProvider?.id ?? null;
    selectedModel.value = firstProvider?.default_model ?? "";
    selectedTemplateId.value = firstTemplate?.id ?? null;

    await chatSessions.loadInitialState();
  } finally {
    chatSessions.loading.value = false;
  }
}

// When a thread is selected via sessions, open its events stream
watch(
  () => chatSessions.activeThreadId.value,
  (threadId) => {
    if (threadId && chatSessions.activeSessionId.value) {
      chatThreadStream.resetRuntimeActivity();
      chatThreadStream.openThreadEvents(chatSessions.activeSessionId.value, threadId);
    }
  },
);

// When active thread changes in sessions, refresh messages
async function handleSelectThreadFromDialog(sessionId: string, threadId: string) {
  chatThreadStream.closeThreadEvents();
  chatSessions.activeSessionId.value = sessionId;
  chatSessions.activeThreadId.value = threadId;
  const session = chatSessions.sessions.value.find((s: ChatSessionSummary) => s.id === sessionId);
  if (session) chatSessions.sessionName.value = session.name || `会话 ${session.id.slice(0, 8)}`;
  // Threads already loaded in dialog - just find the title
  const thread = chatSessions.threads.value.find((t: ChatThreadSummary) => t.id === threadId);
  if (thread) chatSessions.threadTitle.value = thread.title ?? "新的对话线程";
  else chatSessions.threadTitle.value = "新的对话线程";
  await chatThreadStream.refreshActiveThread();
  chatThreadStream.openThreadEvents(sessionId, threadId);
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
    await api.deleteChatSession!(sessionId);
    await chatSessions.refreshSessions();
    chatSessions.activeSessionId.value = "";
    chatSessions.activeThreadId.value = "";
    chatSessions.threads.value = [];
    if (chatSessions.sessions.value.length > 0) {
      await handleSelectThreadFromDialog(
        chatSessions.sessions.value[0].id,
        chatSessions.threads.value[0]?.id ?? "",
      );
    }
  } catch (reason) {
    chatComposer.error.value = reason instanceof Error ? reason.message : String(reason);
  }
}

function handleNewChat() {
  chatThreadStream.closeThreadEvents();
  chatSessions.startNewChatDraft();
  chatComposer.draftMessage.value = "";
  chatThreadStream.messages.value = [];
  chatThreadStream.resetRuntimeActivity();
}

function handleTemplateChange(value: number | null) {
  selectedTemplateId.value = value;
}

function handleProviderChange(value: number | null) {
  selectedProviderId.value = value;
  const provider = providers.value.find((p) => p.id === value);
  if (provider) selectedModel.value = provider.default_model;
}

function handleModelChange(value: string) {
  selectedModel.value = value;
}

function displayMessageContent(message: ChatMessageRecord) {
  const content = message.content?.trim();
  if (content) return message.content;

  if (message.role === "assistant") {
    const names = toolCallNames(message.tool_calls);
    if (names.length > 0) return `正在调用工具：${names.join("、")}`;
    if (message.reasoning_content?.trim()) return "助手正在思考，等待可见回复。";
  }

  if (message.role === "tool" && message.name?.trim()) {
    return `工具 ${message.name} 返回为空。`;
  }

  if (message.role === "tool") return "工具调用结果为空。";
  return "消息内容为空。";
}

function toolCallNames(toolCalls: unknown[] | null | undefined): string[] {
  if (!Array.isArray(toolCalls)) return [];
  return toolCalls
    .map((tc) => {
      if (!tc || typeof tc !== "object" || !("name" in tc)) return "";
      const name = (tc as { name?: unknown }).name;
      return typeof name === "string" ? name.trim() : "";
    })
    .filter((n) => n.length > 0);
}

function applyPrompt(_event: MouseEvent, item: PromptProps) {
  if (item.id === "provider") {
    chatComposer.draftMessage.value = "请检查当前默认模型、提供方和智能体模板是否适合继续这个任务。";
    return;
  }
  if (item.id === "mcp") {
    chatComposer.draftMessage.value = "请帮我梳理当前 MCP 服务的运行风险、可用工具和下一步运维动作。";
    return;
  }
  chatComposer.draftMessage.value = "请基于当前智能体模板，给出系统提示词和工具配置的改进建议。";
}

function runtimeActivityStatusLabel(status: "running" | "success" | "error") {
  if (status === "success") return "完成";
  if (status === "error") return "失败";
  return "运行中";
}
</script>

<template>
  <section class="chat-page">
    <div class="chat-workspace">
      <div class="chat-main-column">
        <!-- Message panel -->
        <article class="chat-panel shell-card">
          <header class="chat-panel__header">
            <div>
              <p class="eyebrow">Conversation</p>
              <h3 class="section-heading">{{ currentConversationTitle }}</h3>
              <p class="section-copy">
                {{ chatSessions.activeBinding.value?.effective_model || selectedModel || "未绑定模型" }}
                <span v-if="activeProvider"> · {{ activeProvider.display_name }}</span>
              </p>
            </div>
            <div class="chat-actions">
              <TinyButton :disabled="!hasActiveThread" @click="chatThreadStream.refreshActiveThread()">刷新</TinyButton>
              <TinyButton :disabled="!hasActiveThread" @click="chatSessions.activateThread()">激活</TinyButton>
              <TinyButton data-testid="cancel-thread" :disabled="!hasActiveThread" @click="chatComposer.cancelThread()">取消运行</TinyButton>
            </div>
          </header>

          <div v-if="chatComposer.error.value" class="notice notice--danger">{{ chatComposer.error.value }}</div>
          <div v-if="chatComposer.actionMessage.value" class="notice notice--success">{{ chatComposer.actionMessage.value }}</div>

          <!-- Runtime activity panel -->
          <div v-if="chatThreadStream.runtimeNotice.value || chatThreadStream.runtimeActivities.value.length > 0" class="runtime-activity-panel">
            <div class="runtime-activity-header">
              <div>
                <p class="eyebrow">Runtime</p>
                <strong>本轮运行活动</strong>
              </div>
              <TinyTag v-if="chatThreadStream.runtimeActivities.value.length > 0" type="info">
                {{ chatThreadStream.runtimeActivities.value.length }} 项
              </TinyTag>
            </div>
            <p v-if="chatThreadStream.runtimeNotice.value" class="runtime-notice">{{ chatThreadStream.runtimeNotice.value }}</p>
            <div v-if="chatThreadStream.runtimeActivities.value.length > 0" class="tool-activity-list">
              <article
                v-for="activity in chatThreadStream.runtimeActivities.value"
                :key="activity.id"
                class="tool-activity-card"
                :class="`tool-activity-card--${activity.status}`"
              >
                <div class="tool-activity-card__header">
                  <strong>{{ activity.name }}</strong>
                  <span>{{ runtimeActivityStatusLabel(activity.status) }}</span>
                </div>
                <pre v-if="activity.argumentsPreview">{{ activity.argumentsPreview }}</pre>
                <pre v-if="activity.resultPreview">{{ activity.resultPreview }}</pre>
              </article>
            </div>
          </div>

          <!-- Message stage -->
          <div class="message-stage">
            <div v-if="chatThreadStream.threadLoading.value && robotMessages.length === 0" class="empty-state">
              正在刷新消息…
            </div>
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

        <!-- Composer bar -->
        <ChatComposerBar
          v-model="chatComposer.draftMessage.value"
          :templates="templates"
          :providers="providers"
          v-model:selected-template-id="selectedTemplateId"
          v-model:selected-provider-id="selectedProviderId"
          v-model:selected-model="selectedModel"
          :disabled="!chatComposer.canSendMessage.value"
          :loading="chatComposer.sending.value"
          :placeholder="chatComposer.senderPlaceholder.value"
          :has-active-thread="hasActiveThread"
          :active-provider="activeProvider"
          :selected-template="selectedTemplate"
          @submit="chatComposer.sendMessage"
          @cancel="chatComposer.cancelThread"
          @new-chat="handleNewChat"
          @open-history="historyDialogOpen = true"
        />
      </div>
    </div>

    <!-- History dialog -->
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
  min-height: 760px;
}

.chat-workspace {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
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
.composer-bar {
  padding: var(--space-5);
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

.notice {
  padding: var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  line-height: 1.5;
}

.notice--danger {
  background: var(--status-danger-bg);
  border: 1px solid var(--status-danger);
  color: var(--status-danger);
}

.notice--success {
  background: var(--status-success-bg);
  border: 1px solid var(--status-success);
  color: var(--status-success);
}

.runtime-activity-panel {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-4);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
}

.runtime-activity-header,
.tool-activity-card__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-3);
}

.runtime-activity-header strong,
.tool-activity-card__header strong {
  color: var(--text-primary);
  font-size: var(--text-sm);
}

.runtime-notice {
  margin: 0;
  color: var(--warning);
  font-size: var(--text-sm);
  line-height: 1.5;
}

.tool-activity-list {
  display: grid;
  gap: var(--space-2);
}

.tool-activity-card {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-3);
  background: var(--surface-overlay);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
}

.tool-activity-card--running {
  border-color: rgba(94, 106, 210, 0.35);
}

.tool-activity-card--success {
  border-color: var(--status-success);
}

.tool-activity-card--error {
  border-color: var(--status-danger);
}

.tool-activity-card__header span {
  color: var(--text-muted);
  font-size: var(--text-xs);
  font-weight: 590;
}

.tool-activity-card pre {
  max-height: 160px;
  margin: 0;
  overflow: auto;
  color: var(--text-secondary);
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  line-height: 1.5;
  white-space: pre-wrap;
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
  background: var(--status-success-bg);
  color: var(--status-success);
}

.chat-avatar--tool {
  background: var(--status-warning-bg);
  color: var(--status-warning);
}

@media (max-width: 1180px) {
  .chat-main-column {
    min-height: 0;
  }

  .message-stage {
    min-height: 420px;
    max-height: none;
  }
}
</style>
