<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";

import {
  getApiClient,
  type AgentRecord,
  type ScheduledMessageRunSummary,
  type ScheduledMessageStatus,
  type ScheduledMessageSummary,
} from "@/lib/api";
import { TinyButton, TinyTag } from "@/lib/opentiny";

const schedules = ref<ScheduledMessageSummary[]>([]);
const router = useRouter();
const templates = ref<AgentRecord[]>([]);
const loading = ref(true);
const actingId = ref("");
const error = ref("");
const actionMessage = ref("");

const summary = computed(() => ({
  total: schedules.value.length,
  pending: schedules.value.filter((item) => item.status === "pending").length,
  running: schedules.value.filter((item) => item.status === "running").length,
  paused: schedules.value.filter((item) => item.status === "paused").length,
  failed: schedules.value.filter((item) => item.status === "failed").length,
}));

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

function scheduleText(item: ScheduledMessageSummary): string {
  if (item.cron_expr) return `cron: ${item.cron_expr}`;
  return item.scheduled_at ? "一次性" : "未设置";
}

function formatDateTime(value: string | null): string {
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

function displayValue(value: string | null): string {
  return value?.trim() || "-";
}

function agentLabel(item: ScheduledMessageSummary): string {
  return templates.value.find((template) => template.id === item.template_id)?.display_name ?? `Agent #${item.template_id}`;
}

function recentTargetText(item: ScheduledMessageSummary): string {
  if (!item.last_session_id || !item.last_thread_id) return "-";
  return `${shortId(item.last_session_id)} / ${shortId(item.last_thread_id)}`;
}

function runHistory(item: ScheduledMessageSummary): ScheduledMessageRunSummary[] {
  if (item.run_history.length > 0) {
    return [...item.run_history]
      .sort((left, right) => Date.parse(right.created_at) - Date.parse(left.created_at))
      .slice(0, 3);
  }
  if (!item.last_session_id || !item.last_thread_id) return [];
  return [
    {
      session_id: item.last_session_id,
      thread_id: item.last_thread_id,
      created_at: item.scheduled_at ?? "",
    },
  ];
}

function schedulerRunHref(item: ScheduledMessageSummary, run: ScheduledMessageRunSummary): string {
  const scheduleId = encodeURIComponent(item.id);
  const session = encodeURIComponent(run.session_id);
  const thread = encodeURIComponent(run.thread_id);
  return `/scheduler/${scheduleId}/runs/${session}/${thread}`;
}

function runHistoryLabel(run: ScheduledMessageRunSummary, index: number): string {
  return `${index === 0 ? "最近" : `#${index + 1}`} ${formatDateTime(run.created_at)}`;
}

async function loadChatOptions() {
  const api = getApiClient();
  if (!api.getChatOptions) {
    error.value = "当前 API 客户端不支持加载 Agent 配置。";
    return;
  }

  try {
    const options = await api.getChatOptions();
    templates.value = options.templates;
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载 Agent 配置失败。";
  }
}

async function loadSchedules(options: { clearError?: boolean } = {}) {
  const api = getApiClient();
  loading.value = true;
  if (options.clearError ?? true) {
    error.value = "";
  }

  if (!api.listScheduledMessages) {
    error.value = "当前 API 客户端不支持 Scheduler。";
    loading.value = false;
    return;
  }

  try {
    schedules.value = await api.listScheduledMessages();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载定时任务失败。";
  } finally {
    loading.value = false;
  }
}

async function pauseSchedule(id: string) {
  const api = getApiClient();
  if (!api.pauseScheduledMessage) {
    error.value = "当前 API 客户端不支持暂停定时任务。";
    return;
  }
  await runAction(id, () => api.pauseScheduledMessage!(id), "定时任务已暂停。");
}

async function triggerSchedule(id: string) {
  const api = getApiClient();
  if (!api.triggerScheduledMessage) {
    error.value = "当前 API 客户端不支持立即触发。";
    return;
  }
  await runAction(id, () => api.triggerScheduledMessage!(id), "定时任务已触发。");
}

async function deleteSchedule(id: string) {
  const api = getApiClient();
  if (!api.deleteScheduledMessage) {
    error.value = "当前 API 客户端不支持删除定时任务。";
    return;
  }
  await runAction(id, () => api.deleteScheduledMessage!(id), "定时任务已删除。");
}

async function runAction(id: string, action: () => Promise<unknown>, message: string) {
  actingId.value = id;
  error.value = "";
  actionMessage.value = "";
  try {
    await action();
    actionMessage.value = message;
    await loadSchedules();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "操作定时任务失败。";
  } finally {
    actingId.value = "";
  }
}

function openCreatePage() {
  void router.push("/scheduler/new");
}

onMounted(() => {
  void (async () => {
    await loadChatOptions();
    await loadSchedules({ clearError: !error.value });
  })();
});
</script>

<template>
  <section class="page-section">
    <div class="page-header">
      <div class="page-header-left">
        <h3 class="page-title">定时任务</h3>
        <TinyTag v-if="!loading">
          {{ schedules.length }} 项
        </TinyTag>
      </div>
      <div class="page-actions">
        <TinyButton
          data-testid="refresh-schedules"
          type="default"
          :disabled="loading"
          @click="loadSchedules"
        >
          {{ loading ? "刷新中" : "刷新" }}
        </TinyButton>
        <TinyButton
          data-testid="create-schedule-link"
          type="primary"
          @click="openCreatePage"
        >
          创建任务
        </TinyButton>
      </div>
    </div>

    <p
      v-if="error"
      class="error-message"
    >
      {{ error }}
    </p>
    <p
      v-if="actionMessage"
      class="success-message"
    >
      {{ actionMessage }}
    </p>

    <div
      v-if="loading"
      class="loading-state"
    >
      加载中...
    </div>

    <div
      v-if="!loading"
      class="ops-grid"
    >
      <article class="ops-card">
        <span class="ops-label">总任务数</span>
        <strong class="ops-value">{{ summary.total }}</strong>
      </article>
      <article class="ops-card">
        <span class="ops-label">待执行</span>
        <strong class="ops-value">{{ summary.pending }}</strong>
      </article>
      <article class="ops-card">
        <span class="ops-label">运行中</span>
        <strong class="ops-value">{{ summary.running }}</strong>
      </article>
      <article class="ops-card">
        <span class="ops-label">已暂停</span>
        <strong class="ops-value">{{ summary.paused }}</strong>
      </article>
      <article class="ops-card">
        <span class="ops-label">失败</span>
        <strong class="ops-value">{{ summary.failed }}</strong>
      </article>
    </div>

    <div
      v-if="!loading && schedules.length === 0"
      class="empty-state"
    >
      暂无定时任务
    </div>

    <section
      v-if="!loading && schedules.length > 0"
      class="scheduler-panel"
    >
      <div class="panel-header">
        <h4>任务列表</h4>
      </div>
      <div class="schedule-table-wrap">
        <table class="schedule-table">
          <colgroup>
            <col class="col-name">
            <col class="col-agent">
            <col class="col-recent">
            <col class="col-status">
            <col class="col-schedule">
            <col class="col-timezone">
            <col class="col-next">
            <col class="col-error">
            <col class="col-actions">
          </colgroup>
          <thead>
            <tr>
              <th>名称</th>
              <th>Agent</th>
              <th>最近对话</th>
              <th>状态</th>
              <th>调度</th>
              <th>时区</th>
              <th>下次执行</th>
              <th>最近错误</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="item in schedules"
              :key="item.id"
            >
              <td
                class="cell-name"
                data-label="名称"
              >
                <strong>{{ item.name }}</strong>
              </td>
              <td data-label="Agent">
                {{ agentLabel(item) }}
              </td>
              <td data-label="最近对话">
                <div
                  v-if="runHistory(item).length > 0"
                  class="history-links"
                >
                  <a
                    v-for="(run, index) in runHistory(item)"
                    :key="`${run.session_id}:${run.thread_id}`"
                    class="history-link"
                    :href="schedulerRunHref(item, run)"
                    :title="`${run.session_id} / ${run.thread_id}`"
                  >
                    {{ runHistoryLabel(run, index) }}
                  </a>
                </div>
                <span
                  v-else
                  class="mono"
                >
                  {{ recentTargetText(item) }}
                </span>
              </td>
              <td data-label="状态">
                <TinyTag :type="statusType(item.status)">
                  {{ statusLabel(item.status) }}
                </TinyTag>
              </td>
              <td data-label="调度">
                <span class="schedule-code">{{ scheduleText(item) }}</span>
              </td>
              <td data-label="时区">
                {{ displayValue(item.timezone) }}
              </td>
              <td data-label="下次执行">
                {{ formatDateTime(item.scheduled_at) }}
              </td>
              <td
                class="error-cell"
                data-label="最近错误"
              >
                {{ displayValue(item.last_error) }}
              </td>
              <td data-label="操作">
                <div class="row-actions">
                  <a
                    class="row-link"
                    :data-testid="`edit-schedule-${item.id}`"
                    :href="`/scheduler/${encodeURIComponent(item.id)}/edit`"
                  >
                    编辑
                  </a>
                  <button
                    class="row-action"
                    :data-testid="`pause-schedule-${item.id}`"
                    :disabled="actingId === item.id || item.status === 'paused'"
                    @click="pauseSchedule(item.id)"
                  >
                    暂停
                  </button>
                  <button
                    class="row-action"
                    :data-testid="`trigger-schedule-${item.id}`"
                    :disabled="actingId === item.id"
                    title="立即触发"
                    @click="triggerSchedule(item.id)"
                  >
                    触发
                  </button>
                  <button
                    class="row-action row-action--danger"
                    :data-testid="`delete-schedule-${item.id}`"
                    :disabled="actingId === item.id"
                    @click="deleteSchedule(item.id)"
                  >
                    删除
                  </button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>
  </section>
</template>

<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.page-header,
.page-header-left,
.panel-header,
.page-actions,
.row-actions {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.page-header {
  justify-content: space-between;
}

.page-title,
.panel-header h4 {
  margin: 0;
  color: var(--text-primary);
}

.page-title {
  font-size: var(--text-base);
  font-weight: 590;
}

.panel-header h4 {
  font-size: var(--text-sm);
  font-weight: 590;
}

.ops-grid {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: var(--space-3);
}

.ops-card,
.scheduler-panel,
.loading-state,
.empty-state {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.ops-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-4);
}

.ops-label {
  font-size: var(--text-xs);
  color: var(--text-muted);
}

.ops-value {
  font-size: var(--text-xl);
  color: var(--text-primary);
}

.scheduler-panel {
  display: grid;
  gap: var(--space-4);
  padding: var(--space-5);
}

.schedule-table-wrap {
  overflow: hidden;
}

.schedule-table {
  width: 100%;
  border-collapse: collapse;
  table-layout: fixed;
}

.col-name {
  width: 14%;
}

.col-agent {
  width: 7%;
}

.col-recent {
  width: 11%;
}

.col-status {
  width: 7%;
}

.col-schedule {
  width: 10%;
}

.col-timezone {
  width: 9%;
}

.col-next {
  width: 13%;
}

.col-error {
  width: 11%;
}

.col-actions {
  width: 18%;
}

.schedule-table th,
.schedule-table td {
  padding: var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
  text-align: left;
  vertical-align: middle;
  font-size: var(--text-sm);
}

.schedule-table th {
  color: var(--text-muted);
  font-size: var(--text-xs);
  font-weight: 590;
  white-space: nowrap;
}

.schedule-table td {
  min-width: 0;
  color: var(--text-primary);
  line-height: 1.4;
}

.cell-name strong {
  display: -webkit-box;
  overflow: hidden;
  -webkit-box-orient: vertical;
}

.cell-name strong {
  -webkit-line-clamp: 2;
}

.error-cell {
  overflow-wrap: anywhere;
}

.schedule-code {
  display: block;
  overflow: hidden;
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.history-links {
  display: grid;
  gap: var(--space-1);
}

.history-link {
  overflow: hidden;
  color: var(--color-primary);
  font-size: var(--text-xs);
  text-decoration: none;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.history-link:hover {
  text-decoration: underline;
}

.mono {
  display: block;
  overflow: hidden;
  color: var(--text-muted);
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.error-cell {
  color: var(--status-danger);
}

.row-actions {
  align-items: center;
  flex-wrap: nowrap;
  gap: var(--space-3);
  min-width: 0;
}

.row-link,
.row-action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: var(--text-xs);
  font-weight: 590;
  text-decoration: none;
}

.page-actions :deep(.tiny-button) {
  height: 32px !important;
  min-height: 32px !important;
  padding: 0 var(--space-3) !important;
  box-sizing: border-box !important;
  font-size: var(--text-xs) !important;
  font-weight: 590 !important;
  line-height: 1 !important;
}

.row-link,
.row-action {
  width: auto;
  min-width: 0;
  height: auto;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--accent);
  cursor: pointer;
  line-height: 1.4;
}

.row-link:hover,
.row-action:hover {
  color: var(--accent-hover);
  text-decoration: underline;
}

.row-action:disabled {
  color: var(--text-placeholder);
  cursor: not-allowed;
  text-decoration: none;
}

.row-action--danger {
  color: var(--danger);
}

.loading-state,
.empty-state {
  padding: var(--space-10) var(--space-4);
  color: var(--text-muted);
  text-align: center;
}

.error-message,
.success-message {
  margin: 0;
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}

.error-message {
  background: var(--status-danger-bg);
  color: var(--status-danger);
}

.success-message {
  background: var(--status-success-bg);
  color: var(--status-success);
}

@media (max-width: 960px) {
  .ops-grid {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 1180px) {
  .schedule-table-wrap {
    overflow: visible;
  }

  .schedule-table,
  .schedule-table thead,
  .schedule-table tbody,
  .schedule-table tr,
  .schedule-table td {
    display: block;
    width: 100%;
  }

  .schedule-table {
    min-width: 0;
  }

  .schedule-table thead,
  .schedule-table colgroup {
    display: none;
  }

  .schedule-table tr {
    padding: var(--space-4) 0;
    border-bottom: 1px solid var(--border-subtle);
  }

  .schedule-table tr:last-child {
    border-bottom: 0;
  }

  .schedule-table td {
    display: grid;
    grid-template-columns: 108px minmax(0, 1fr);
    gap: var(--space-3);
    padding: var(--space-2) var(--space-1);
    border-bottom: 0;
  }

  .schedule-table td::before {
    color: var(--text-muted);
    content: attr(data-label);
    font-size: var(--text-xs);
    font-weight: 590;
  }

  .row-actions {
    justify-content: flex-start;
  }
}
</style>
