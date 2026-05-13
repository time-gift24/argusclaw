<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";

import ChatMessageStage from "@/features/chat/components/ChatMessageStage.vue";
import { createBubbleRoles, toRobotMessages } from "@/features/chat/composables/useChatPresentation";
import {
  getApiClient,
  type ChatMessageRecord,
  type ScheduledMessageRunSummary,
  type ScheduledMessageStatus,
  type ScheduledMessageSummary,
} from "@/lib/api";
import { TinyTag } from "@/lib/opentiny";

const route = useRoute();
const loading = ref(true);
const error = ref("");
const schedule = ref<ScheduledMessageSummary | null>(null);
const run = ref<ScheduledMessageRunSummary | null>(null);
const messages = ref<ChatMessageRecord[]>([]);
const bubbleRoles = createBubbleRoles();

const scheduleId = computed(() => routeParam("scheduleId"));
const sessionId = computed(() => routeParam("sessionId"));
const threadId = computed(() => routeParam("threadId"));
const robotMessages = computed(() =>
  toRobotMessages({
    messages: messages.value,
    streaming: false,
    hasActiveThread: true,
    pendingAssistantContent: "",
    pendingAssistantReasoning: "",
    runtimeActivities: [],
    pendingTimeline: [],
  }),
);

function routeParam(name: string): string {
  const value = route.params[name];
  if (Array.isArray(value)) return value[0] ?? "";
  return value ? String(value) : "";
}

function statusType(status: ScheduledMessageStatus): "success" | "info" | "warning" | "danger" {
  if (status === "succeeded") return "success";
  if (status === "failed" || status === "cancelled") return "danger";
  if (status === "running") return "warning";
  return "info";
}

function statusLabel(status: ScheduledMessageStatus): string {
  return {
    pending: "待执行",
    queued: "排队中",
    running: "运行中",
    succeeded: "已完成",
    failed: "失败",
    cancelled: "已取消",
    paused: "已暂停",
  }[status];
}

function scheduleText(item: ScheduledMessageSummary | null): string {
  if (!item) return "-";
  if (item.cron_expr) return `cron: ${item.cron_expr}`;
  return item.scheduled_at ? "一次性" : "未设置";
}

function formatDateTime(value: string | null | undefined): string {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    month: "2-digit",
    day: "2-digit",
    year: "numeric",
  });
}

function shortId(value: string): string {
  return value.length > 12 ? `${value.slice(0, 8)}...${value.slice(-4)}` : value;
}

function findRun(item: ScheduledMessageSummary): ScheduledMessageRunSummary | null {
  const historyRun = item.run_history.find(
    (candidate) => candidate.session_id === sessionId.value && candidate.thread_id === threadId.value,
  );
  if (historyRun) return historyRun;
  return null;
}

async function loadRun() {
  const api = getApiClient();
  loading.value = true;
  error.value = "";

  if (!api.listScheduledMessages || !api.listChatMessages) {
    error.value = "当前 API 客户端不支持查看 Scheduler 运行记录。";
    loading.value = false;
    return;
  }

  try {
    const schedules = await api.listScheduledMessages();
    const matchedSchedule = schedules.find((item) => item.id === scheduleId.value) ?? null;
    if (!matchedSchedule) {
      error.value = "未找到这个 Scheduler 任务。";
      return;
    }

    const matchedRun = findRun(matchedSchedule);
    if (!matchedRun) {
      error.value = "未找到这次 Scheduler 运行记录。";
      schedule.value = matchedSchedule;
      return;
    }

    schedule.value = matchedSchedule;
    run.value = matchedRun;
    messages.value = await api.listChatMessages(sessionId.value, threadId.value);
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载 Scheduler 运行记录失败。";
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  void loadRun();
});
</script>

<template>
  <section class="scheduler-run-page">
    <header class="run-header">
      <div class="run-header__content">
        <span class="eyebrow">定时任务运行记录</span>
        <h3>{{ schedule?.name ?? "运行记录" }}</h3>
        <p>
          {{ formatDateTime(run?.created_at) }}
          <span v-if="run"> · {{ shortId(run.session_id) }} / {{ shortId(run.thread_id) }}</span>
        </p>
      </div>
      <a class="back-link" href="/scheduler">返回定时任务</a>
    </header>

    <p v-if="error" class="error-message">
      {{ error }}
    </p>

    <div v-if="loading" class="loading-state">
      加载中...
    </div>

    <template v-else>
      <section v-if="schedule" class="run-meta-grid" aria-label="运行元信息">
        <article class="meta-item">
          <span>任务状态</span>
          <TinyTag :type="statusType(schedule.status)">
            {{ statusLabel(schedule.status) }}
          </TinyTag>
        </article>
        <article class="meta-item">
          <span>调度</span>
          <strong>{{ scheduleText(schedule) }}</strong>
        </article>
        <article class="meta-item">
          <span>时区</span>
          <strong>{{ schedule.timezone || "-" }}</strong>
        </article>
        <article class="meta-item">
          <span>执行时间</span>
          <strong>{{ formatDateTime(run?.created_at) }}</strong>
        </article>
      </section>

      <section class="message-panel">
        <div class="panel-header">
          <h4>对话内容</h4>
        </div>
        <div v-if="messages.length === 0 && !error" class="empty-state">
          这次运行还没有可展示的消息。
        </div>
        <ChatMessageStage
          v-else-if="messages.length > 0"
          :loading="false"
          :messages="robotMessages"
          :bubble-roles="bubbleRoles"
          :starter-prompts="[]"
        />
      </section>
    </template>
  </section>
</template>

<style scoped>
.scheduler-run-page {
  display: grid;
  gap: var(--space-5);
}

.run-header,
.run-meta-grid,
.message-panel,
.loading-state {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.run-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-5);
}

.run-header__content {
  display: grid;
  gap: var(--space-2);
  min-width: 0;
}

.eyebrow {
  color: var(--text-muted);
  font-size: var(--text-xs);
  font-weight: 590;
}

.run-header h3,
.panel-header h4 {
  margin: 0;
  color: var(--text-primary);
  font-weight: 590;
}

.run-header h3 {
  overflow-wrap: anywhere;
  font-size: var(--text-lg);
}

.run-header p {
  margin: 0;
  color: var(--text-secondary);
  font-family: var(--font-mono);
  font-size: var(--text-xs);
}

.back-link {
  flex: 0 0 auto;
  color: var(--accent);
  font-size: var(--text-sm);
  text-decoration: none;
}

.back-link:hover {
  text-decoration: underline;
}

.run-meta-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 0;
  overflow: hidden;
}

.meta-item {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-4);
  border-right: 1px solid var(--border-subtle);
}

.meta-item:last-child {
  border-right: 0;
}

.meta-item span {
  color: var(--text-muted);
  font-size: var(--text-xs);
}

.meta-item strong {
  overflow-wrap: anywhere;
  color: var(--text-primary);
  font-size: var(--text-sm);
  font-weight: 590;
}

.message-panel {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-4);
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.panel-header h4 {
  font-size: var(--text-sm);
}

.message-panel :deep(.message-stage) {
  padding: 0 0 var(--space-1);
}

.message-panel :deep(.tr-bubble-item) {
  margin-bottom: var(--space-2);
}

.message-panel :deep(.tr-bubble-list),
.message-panel :deep(.bubble-list) {
  gap: var(--space-2);
}

.message-panel :deep(.tr-bubble__box[data-role="user"]) {
  padding: var(--space-2) var(--space-3) !important;
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__reasoning) {
  margin-bottom: var(--space-2);
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .header) {
  gap: var(--space-2);
  margin-bottom: var(--space-2);
  font-size: var(--text-xs);
  line-height: 1.5;
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .title) {
  font-size: var(--text-xs);
  line-height: 1.5;
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .icon-and-text) {
  gap: var(--space-1);
  padding: 2px 8px;
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .detail) {
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-lg);
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .detail-content),
.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown),
.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown p),
.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown li),
.message-panel :deep(.tr-bubble__box[data-role="user"]),
.message-panel :deep(.tr-bubble__box[data-role="user"] .tr-bubble__markdown),
.message-panel :deep(.tr-bubble__box[data-role="user"] .tr-bubble__markdown p) {
  font-size: var(--text-xs);
  line-height: 1.55;
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .detail-content),
.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown p),
.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown ul),
.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown ol) {
  margin: 0 0 var(--space-2);
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown h1),
.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown h2),
.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown h3) {
  margin: var(--space-3) 0 var(--space-1);
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown h1) {
  font-size: var(--text-base);
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown h2),
.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown h3) {
  font-size: var(--text-sm);
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown pre) {
  margin: var(--space-2) 0;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-lg);
  font-size: var(--text-xs);
  line-height: 1.55;
}

.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown th),
.message-panel :deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown td) {
  padding: 6px 8px;
}

.message-panel :deep(.chat-avatar) {
  min-width: 24px;
  height: 24px;
  padding: 0 6px;
  font-size: 11px;
}

.loading-state,
.empty-state {
  padding: var(--space-10) var(--space-4);
  color: var(--text-muted);
  text-align: center;
}

.error-message {
  margin: 0;
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md);
  background: var(--status-danger-bg);
  color: var(--status-danger);
  font-size: var(--text-sm);
}

@media (max-width: 960px) {
  .run-header {
    display: grid;
  }

  .run-meta-grid {
    grid-template-columns: 1fr;
  }

  .meta-item {
    border-right: 0;
    border-bottom: 1px solid var(--border-subtle);
  }

  .meta-item:last-child {
    border-bottom: 0;
  }
}
</style>
