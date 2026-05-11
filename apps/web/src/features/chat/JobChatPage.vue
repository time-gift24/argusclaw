<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import type { PromptProps } from "@opentiny/tiny-robot";

import { getApiClient, type ChatJobConversation } from "@/lib/api";
import ChatConversationPanel from "./components/ChatConversationPanel.vue";
import {
  createBubbleRoles,
  toRobotMessages,
} from "./composables/useChatPresentation";

const route = useRoute();
const router = useRouter();

const conversation = ref<ChatJobConversation | null>(null);
const loading = ref(false);
const error = ref("");
const requestSequence = ref(0);
const bubbleRoles = createBubbleRoles();
const starterPrompts: PromptProps[] = [];
const POLL_INTERVAL_MS = 1500;
const ACTIVE_STATUSES = new Set(["pending", "queued", "running"]);
let pollTimer: ReturnType<typeof setInterval> | null = null;
let pollInFlight = false;

const jobId = computed(() => {
  const value = route.params.jobId;
  if (Array.isArray(value)) {
    return typeof value[0] === "string" ? value[0] : "";
  }
  return typeof value === "string" ? value : "";
});

const robotMessages = computed(() => toRobotMessages({
  messages: conversation.value?.messages ?? [],
  streaming: false,
  hasActiveThread: Boolean(conversation.value?.thread_id),
  pendingAssistantContent: "",
  pendingAssistantReasoning: "",
  runtimeActivities: [],
}));

const notice = computed(() => {
  if (conversation.value && !conversation.value.thread_id) {
    return "Job 已创建，执行线程尚未就绪。";
  }
  return "";
});

const hasMessages = computed(() => robotMessages.value.length > 0);
const emptyStateText = computed(() => {
  if (loading.value) return "正在加载 Job 对话…";
  if (error.value) return error.value;
  if (notice.value) return notice.value;
  return "Job 对话尚未写入消息。";
});

watch(jobId, (nextJobId) => {
  void loadConversation(nextJobId);
}, { immediate: true });

onBeforeUnmount(() => {
  stopPolling();
  requestSequence.value += 1;
});

async function loadConversation(nextJobId: string) {
  stopPolling();
  const requestId = requestSequence.value + 1;
  requestSequence.value = requestId;
  loading.value = true;
  error.value = "";
  conversation.value = null;

  if (!nextJobId) {
    error.value = "Job ID 无效。";
    loading.value = false;
    return;
  }

  try {
    const api = getApiClient();
    if (!api.getChatJobConversation) {
      throw new Error("当前服务不支持 Job 对话。");
    }
    const nextConversation = await api.getChatJobConversation(nextJobId);
    if (requestId !== requestSequence.value) return;
    conversation.value = nextConversation;
    updatePollingForConversation(nextConversation);
  } catch (reason) {
    if (requestId !== requestSequence.value) return;
    error.value = formatErrorMessage(reason);
  } finally {
    if (requestId === requestSequence.value) {
      loading.value = false;
    }
  }
}

function updatePollingForConversation(nextConversation: ChatJobConversation) {
  if (isActiveJobStatus(nextConversation.status)) {
    startPolling();
    return;
  }
  stopPolling();
}

function isActiveJobStatus(status: string) {
  return ACTIVE_STATUSES.has(status);
}

function startPolling() {
  if (pollTimer) return;
  pollTimer = setInterval(() => {
    void refreshConversation();
  }, POLL_INTERVAL_MS);
}

function stopPolling() {
  if (!pollTimer) return;
  clearInterval(pollTimer);
  pollTimer = null;
}

async function refreshConversation() {
  if (pollInFlight) return;

  const nextJobId = jobId.value;
  if (!nextJobId) {
    stopPolling();
    return;
  }

  const requestId = requestSequence.value;
  pollInFlight = true;

  try {
    const api = getApiClient();
    if (!api.getChatJobConversation) {
      throw new Error("当前服务不支持 Job 对话。");
    }
    const nextConversation = await api.getChatJobConversation(nextJobId);
    if (requestId !== requestSequence.value || nextJobId !== jobId.value) return;
    error.value = "";
    conversation.value = nextConversation;
    updatePollingForConversation(nextConversation);
  } catch (reason) {
    if (requestId !== requestSequence.value || nextJobId !== jobId.value) return;
    error.value = formatErrorMessage(reason);
  } finally {
    pollInFlight = false;
  }
}

function firstQueryValue(value: unknown) {
  if (typeof value === "string") return value;
  if (Array.isArray(value) && typeof value[0] === "string") return value[0];
  return null;
}

function returnToParentConversation() {
  const parentSession = conversation.value?.parent_session_id;
  const parentThread = conversation.value?.parent_thread_id;
  if (typeof parentSession === "string" && typeof parentThread === "string") {
    void router.push({ name: "chat", query: { session: parentSession, thread: parentThread } });
    return;
  }

  const querySession = firstQueryValue(route.query.fromSession);
  const queryThread = firstQueryValue(route.query.fromThread);
  if (typeof querySession === "string" && typeof queryThread === "string") {
    void router.push({ name: "chat", query: { session: querySession, thread: queryThread } });
    return;
  }

  void router.push({ name: "chat" });
}

function formatErrorMessage(reason: unknown) {
  if (reason instanceof Error && reason.message) return reason.message;
  return "Job 对话加载失败。";
}
</script>

<template>
  <section class="job-chat-page">
    <header class="job-chat-page__topbar">
      <div class="job-chat-page__heading">
        <p class="job-chat-page__breadcrumb">对话 / Job {{ jobId }}</p>
        <h1>{{ conversation?.title || `Job ${jobId}` }}</h1>
        <p class="job-chat-page__meta">
          状态 {{ conversation?.status ?? "加载中" }}
          <span v-if="conversation"> · {{ conversation.turn_count }} turns · {{ conversation.token_count }} tokens</span>
        </p>
      </div>
      <button class="job-chat-page__back" type="button" @click="returnToParentConversation">
        返回父对话
      </button>
    </header>

    <div class="job-chat-page__body">
      <ChatConversationPanel
        v-if="hasMessages"
        :error="error"
        :notice="notice"
        :thread-loading="loading"
        :robot-messages="robotMessages"
        :bubble-roles="bubbleRoles"
        :starter-prompts="starterPrompts"
      />
      <div v-else class="job-chat-page__empty" :class="{ 'job-chat-page__empty--danger': error }">
        {{ emptyStateText }}
      </div>
    </div>
  </section>
</template>

<style scoped>
.job-chat-page {
  --chat-message-width: 1120px;
  min-height: 100vh;
  width: calc(100% + var(--space-6) + var(--space-6));
  margin: calc(0px - var(--space-6));
  background: var(--app-bg);
}

.job-chat-page__topbar {
  position: sticky;
  top: 0;
  z-index: 20;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4) max(var(--space-6), calc((100% - var(--chat-message-width)) / 2));
  background: color-mix(in srgb, var(--app-bg) 92%, transparent);
  border-bottom: 1px solid var(--border-subtle);
  backdrop-filter: blur(14px);
}

.job-chat-page__heading {
  min-width: 0;
}

.job-chat-page__breadcrumb,
.job-chat-page__meta {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--text-sm);
}

.job-chat-page__heading h1 {
  margin: var(--space-1) 0;
  color: var(--text-primary);
  font-size: var(--text-xl);
  font-weight: 650;
  letter-spacing: 0;
}

.job-chat-page__back {
  flex: 0 0 auto;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-base);
  color: var(--text-primary);
  cursor: pointer;
  font-size: var(--text-sm);
  font-weight: 590;
  padding: var(--space-2) var(--space-3);
}

.job-chat-page__back:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.job-chat-page__body {
  display: flex;
  flex-direction: column;
  min-height: calc(100vh - 86px);
  padding: var(--space-6) max(var(--space-6), calc((100% - var(--chat-message-width)) / 2)) 0;
}

.job-chat-page__body :deep(.message-stage) {
  padding-bottom: var(--space-6);
}

.job-chat-page__empty {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 280px;
  border: 1px dashed var(--border-default);
  border-radius: var(--radius-lg);
  background: var(--surface-base);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  line-height: 1.5;
  padding: var(--space-6);
  text-align: center;
}

.job-chat-page__empty--danger {
  border-color: color-mix(in srgb, var(--status-danger) 42%, var(--border-default));
  color: var(--status-danger);
}

@media (max-width: 1180px) {
  .job-chat-page {
    width: calc(100% + var(--space-4) + var(--space-4));
    margin: calc(0px - var(--space-4));
  }

  .job-chat-page__topbar,
  .job-chat-page__body {
    padding-right: var(--space-4);
    padding-left: var(--space-4);
  }
}

@media (max-width: 760px) {
  .job-chat-page__topbar {
    align-items: stretch;
    flex-direction: column;
  }

  .job-chat-page__back {
    width: 100%;
  }
}
</style>
