<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
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
const bubbleRoles = createBubbleRoles();
const starterPrompts: PromptProps[] = [];

const jobId = computed(() => {
  const value = route.params.jobId;
  return Array.isArray(value) ? value[0] ?? "" : value;
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

onMounted(() => {
  void loadConversation();
});

async function loadConversation() {
  loading.value = true;
  error.value = "";
  try {
    const api = getApiClient();
    if (!api.getChatJobConversation) {
      throw new Error("当前服务不支持 Job 对话。");
    }
    conversation.value = await api.getChatJobConversation(jobId.value);
  } catch (reason) {
    error.value = formatErrorMessage(reason);
  } finally {
    loading.value = false;
  }
}

function firstQueryValue(value: unknown) {
  if (typeof value === "string") return value;
  if (Array.isArray(value) && typeof value[0] === "string") return value[0];
  return null;
}

function returnToParentConversation() {
  const session = conversation.value?.parent_session_id ?? firstQueryValue(route.query.fromSession);
  const thread = conversation.value?.parent_thread_id ?? firstQueryValue(route.query.fromThread);

  if (typeof session === "string" && typeof thread === "string") {
    void router.push({ name: "chat", query: { session, thread } });
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
        :error="error"
        :notice="notice"
        :thread-loading="loading"
        :robot-messages="robotMessages"
        :bubble-roles="bubbleRoles"
        :starter-prompts="starterPrompts"
      />
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
