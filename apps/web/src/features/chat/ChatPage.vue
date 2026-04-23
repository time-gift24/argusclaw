<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import { TrBubbleList, TrPrompts, TrSender, type BubbleRoleConfig, type PromptProps } from "@opentiny/tiny-robot";

import {
  getApiClient,
  type AgentRecord,
  type ApiClient,
  type ChatMessageRecord,
  type ChatSessionSummary,
  type ChatThreadBinding,
  type ChatThreadSummary,
  type LlmProviderRecord,
} from "@/lib/api";
import { TinyButton, TinyInput, TinyOption, TinySelect, TinyTag } from "@/lib/opentiny";

const api = getApiClient();
type ChatApiMethods = Required<
  Pick<
    ApiClient,
    | "listChatSessions"
    | "createChatSession"
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
const creatingSession = ref(false);
const creatingThread = ref(false);
const deleting = ref(false);
const streaming = ref(false);
const actionMessage = ref("");
const error = ref("");
const activeBinding = ref<ChatThreadBinding | null>(null);

const activeSession = computed(() => sessions.value.find((session) => session.id === activeSessionId.value) ?? null);
const activeThread = computed(() => threads.value.find((thread) => thread.id === activeThreadId.value) ?? null);
const canCreateThread = computed(() => Boolean(activeSession.value && selectedTemplateId.value));
const activeProvider = computed(
  () => providers.value.find((provider) => provider.id === Number(selectedProviderId.value)) ?? null,
);
const senderPlaceholder = computed(() => {
  if (!activeThread.value) {
    return "先创建或选择一个线程";
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
      content: message.content || emptyContentForRole(message.role),
    }));

  if (streaming.value && activeThread.value) {
    nextMessages.push({
      role: "assistant",
      content: "正在生成回复…",
    });
  }

  return nextMessages;
});

const stats = computed(() => ({
  sessions: sessions.value.length,
  threads: threads.value.length,
  messages: messages.value.length,
}));

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

async function loadInitialState() {
  loading.value = true;
  error.value = "";
  try {
    const [nextProviders, nextTemplates, nextSessions] = await Promise.all([
      api.listProviders(),
      api.listTemplates(),
      callChatApi("listChatSessions"),
    ]);

    providers.value = nextProviders;
    templates.value = nextTemplates;
    sessions.value = nextSessions;
    selectDefaultTemplateAndProvider();

    if (nextSessions.length > 0) {
      await selectSession(nextSessions[0].id);
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
  activeSessionId.value = sessionId;
  activeThreadId.value = "";
  messages.value = [];
  activeBinding.value = null;
  const session = sessions.value.find((item) => item.id === sessionId);
  if (session) {
    sessionName.value = session.name;
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
  activeThreadId.value = threadId;
  const thread = threads.value.find((item) => item.id === threadId);
  threadTitle.value = thread?.title ?? "新的对话线程";
  await refreshActiveThread();
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

async function createSession() {
  creatingSession.value = true;
  try {
    await runAction(async () => {
      const created = await callChatApi("createChatSession", sessionName.value || "新的 Web 对话");
      await refreshSessions();
      if (!sessions.value.some((session) => session.id === created.id)) {
        sessions.value = [created, ...sessions.value];
      }
      await selectSession(created.id);
    }, "已创建新的对话会话。");
  } finally {
    creatingSession.value = false;
  }
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
  if (!activeSessionId.value || !activeThreadId.value || !content || sending.value) {
    return;
  }

  sending.value = true;
  streaming.value = true;
  const assistantCountBeforeSend = countAssistantMessages();
  messages.value = [...messages.value, createLocalMessage("user", content)];
  try {
    await runAction(async () => {
      await callChatApi("sendChatMessage", activeSessionId.value, activeThreadId.value, content);
      draftMessage.value = "";
      void refreshStreamUntilSettled(assistantCountBeforeSend);
    }, "消息已提交，正在等待流式结果。");
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
    await callChatApi("cancelChatThread", activeSessionId.value, activeThreadId.value);
  }, "已请求取消当前线程。");
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
  error.value = reason instanceof Error ? reason.message : String(reason);
}

async function refreshStreamUntilSettled(assistantCountBeforeSend: number) {
  for (let attempt = 0; attempt < 8; attempt += 1) {
    if (!streaming.value || !activeSessionId.value || !activeThreadId.value) {
      return;
    }

    await refreshActiveThread({ silent: true });
    if (countAssistantMessages() > assistantCountBeforeSend) {
      streaming.value = false;
      actionMessage.value = "回复已刷新。";
      return;
    }

    await waitForStreamTick();
  }

  streaming.value = false;
  actionMessage.value = "消息已提交，后端仍在处理；可点击刷新获取最新回复。";
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
            <TinyInput v-model="sessionName" name="session-name" placeholder="新的 Web 对话" />
            <TinyButton data-testid="create-session" :disabled="creatingSession" type="primary" @click="createSession">
              {{ creatingSession ? "创建中" : "新建" }}
            </TinyButton>
          </div>

          <div v-if="loading" class="empty-state">正在加载对话会话…</div>
          <div v-else-if="sessions.length === 0" class="empty-state">暂无对话会话，先创建一个 Web 对话。</div>
          <div v-else class="session-list">
            <button
              v-for="sessionItem in sessions"
              :key="sessionItem.id"
              class="session-item"
              :class="{ active: sessionItem.id === activeSessionId }"
              type="button"
              @click="selectSession(sessionItem.id)"
            >
              <span>{{ sessionItem.name }}</span>
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
              <span>智能体模板</span>
              <TinySelect v-model="selectedTemplateId" name="template">
                <TinyOption v-for="item in templates" :key="item.id" :label="item.display_name" :value="item.id" />
              </TinySelect>
            </label>
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

          <div v-if="!activeSession" class="empty-state">请选择或创建一个会话。</div>
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

          <div v-if="activeThread" class="thread-editor">
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
              <h3 class="section-heading">{{ activeThread ? formatThreadTitle(activeThread) : "未选择线程" }}</h3>
              <p class="section-copy">
                {{ activeBinding?.effective_model || selectedModel || "未绑定模型" }}
                <span v-if="activeProvider"> · {{ activeProvider.display_name }}</span>
              </p>
            </div>
            <div class="chat-actions">
              <TinyButton :disabled="!activeThread" @click="refreshActiveThread">刷新</TinyButton>
              <TinyButton :disabled="!activeThread" @click="activateThread">激活</TinyButton>
              <TinyButton data-testid="cancel-thread" :disabled="!activeThread" @click="cancelThread">取消运行</TinyButton>
            </div>
          </header>

          <div v-if="error" class="notice notice--danger">{{ error }}</div>
          <div v-if="actionMessage" class="notice notice--success">{{ actionMessage }}</div>

          <div class="message-stage">
            <div v-if="threadLoading" class="empty-state">正在刷新消息…</div>
            <div v-else-if="!activeThread" class="empty-state">创建线程后即可开始对话。</div>
            <div v-else-if="robotMessages.length === 0" class="prompt-panel">
              <p class="prompt-title">快速开始</p>
              <TrPrompts :items="starterPrompts" wrap @item-click="applyPrompt" />
            </div>
            <TrBubbleList
              v-else
              class="bubble-list"
              :messages="robotMessages"
              :role-configs="bubbleRoles"
              auto-scroll
              group-strategy="divider"
            />
          </div>
        </article>

        <footer class="composer-panel shell-card">
          <TrSender
            v-model="draftMessage"
            class="chat-sender"
            :clearable="true"
            :disabled="!activeThread"
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

.thread-config {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.thread-config label {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 590;
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
