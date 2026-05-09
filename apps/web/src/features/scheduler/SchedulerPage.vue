<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";

import {
  getApiClient,
  type CreateScheduledMessageRequest,
  type ScheduledMessageStatus,
  type ScheduledMessageSummary,
} from "@/lib/api";
import { TinyButton, TinyInput, TinyOption, TinySelect, TinyTag } from "@/lib/opentiny";

type ScheduleMode = "cron" | "once";

const schedules = ref<ScheduledMessageSummary[]>([]);
const loading = ref(true);
const submitting = ref(false);
const actingId = ref("");
const error = ref("");
const actionMessage = ref("");
const scheduleMode = ref<ScheduleMode>("cron");
const form = reactive({
  sessionId: "",
  threadId: "",
  name: "",
  prompt: "",
  cronExpr: "0 9 * * *",
  timezone: "Asia/Shanghai",
  scheduledAt: "",
});

const summary = computed(() => ({
  total: schedules.value.length,
  pending: schedules.value.filter((item) => item.status === "pending").length,
  running: schedules.value.filter((item) => item.status === "running").length,
  paused: schedules.value.filter((item) => item.status === "paused").length,
  failed: schedules.value.filter((item) => item.status === "failed").length,
}));

const canSubmit = computed(() => {
  const hasTarget = form.sessionId.trim() && form.threadId.trim();
  const hasPrompt = form.prompt.trim();
  const hasSchedule = scheduleMode.value === "cron" ? form.cronExpr.trim() : form.scheduledAt.trim();
  return Boolean(hasTarget && hasPrompt && hasSchedule && !submitting.value);
});

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

function displayValue(value: string | null): string {
  return value?.trim() || "-";
}

async function loadSchedules() {
  const api = getApiClient();
  loading.value = true;
  error.value = "";

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

async function createSchedule() {
  const api = getApiClient();
  if (!api.createScheduledMessage) {
    error.value = "当前 API 客户端不支持创建定时任务。";
    return;
  }
  if (!canSubmit.value) {
    error.value = "请填写目标对话、提示词和调度配置。";
    return;
  }

  const input: CreateScheduledMessageRequest = {
    session_id: form.sessionId.trim(),
    thread_id: form.threadId.trim(),
    name: form.name.trim() || "Scheduled message",
    prompt: form.prompt.trim(),
  };
  if (scheduleMode.value === "cron") {
    input.cron_expr = form.cronExpr.trim();
    input.timezone = form.timezone.trim() || null;
  } else {
    input.scheduled_at = form.scheduledAt.trim();
  }

  submitting.value = true;
  error.value = "";
  actionMessage.value = "";
  try {
    await api.createScheduledMessage(input);
    actionMessage.value = "定时任务已创建。";
    resetForm();
    await loadSchedules();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "创建定时任务失败。";
  } finally {
    submitting.value = false;
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

function resetForm() {
  form.name = "";
  form.prompt = "";
  form.cronExpr = "0 9 * * *";
  form.timezone = "Asia/Shanghai";
  form.scheduledAt = "";
}

onMounted(() => {
  void loadSchedules();
});
</script>

<template>
  <section class="page-section">
    <div class="page-header">
      <div class="page-header-left">
        <h3 class="page-title">Scheduler</h3>
        <TinyTag v-if="!loading">
          {{ schedules.length }} 项
        </TinyTag>
      </div>
      <TinyButton
        data-testid="refresh-schedules"
        type="default"
        :disabled="loading"
        @click="loadSchedules"
      >
        {{ loading ? "刷新中" : "刷新" }}
      </TinyButton>
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

    <section class="scheduler-panel">
      <div class="panel-header">
        <h4>创建定时任务</h4>
      </div>
      <div class="form-grid">
        <label>
          <span>Session ID</span>
          <TinyInput
            v-model="form.sessionId"
            data-testid="schedule-session-id"
            placeholder="session uuid"
          />
        </label>
        <label>
          <span>Thread ID</span>
          <TinyInput
            v-model="form.threadId"
            data-testid="schedule-thread-id"
            placeholder="thread uuid"
          />
        </label>
        <label>
          <span>任务名称</span>
          <TinyInput
            v-model="form.name"
            data-testid="schedule-name"
            placeholder="每日检查"
          />
        </label>
        <label>
          <span>调度类型</span>
          <TinySelect
            v-model="scheduleMode"
            data-testid="schedule-mode"
          >
            <TinyOption
              label="Cron"
              value="cron"
            />
            <TinyOption
              label="一次性"
              value="once"
            />
          </TinySelect>
        </label>
        <label class="form-wide">
          <span>提示词</span>
          <TinyInput
            v-model="form.prompt"
            data-testid="schedule-prompt"
            type="textarea"
            placeholder="到点后发送到目标 thread 的用户消息"
          />
        </label>
        <template v-if="scheduleMode === 'cron'">
          <label>
            <span>Cron 表达式</span>
            <TinyInput
              v-model="form.cronExpr"
              data-testid="schedule-cron-expr"
              placeholder="0 9 * * *"
            />
          </label>
          <label>
            <span>时区</span>
            <TinyInput
              v-model="form.timezone"
              data-testid="schedule-timezone"
              placeholder="Asia/Shanghai"
            />
          </label>
        </template>
        <label
          v-else
          class="form-wide"
        >
          <span>一次性时间</span>
          <TinyInput
            v-model="form.scheduledAt"
            data-testid="schedule-scheduled-at"
            placeholder="2026-05-10T01:00:00Z"
          />
        </label>
      </div>
      <div class="form-actions">
        <TinyButton
          data-testid="create-schedule"
          type="primary"
          :disabled="!canSubmit"
          @click="createSchedule"
        >
          {{ submitting ? "创建中" : "创建任务" }}
        </TinyButton>
      </div>
    </section>

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
          <thead>
            <tr>
              <th>名称</th>
              <th>目标</th>
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
              <td>
                <strong>{{ item.name }}</strong>
              </td>
              <td>
                <span class="mono">{{ item.session_id }}</span>
                <span class="mono">{{ item.thread_id }}</span>
              </td>
              <td>
                <TinyTag :type="statusType(item.status)">
                  {{ statusLabel(item.status) }}
                </TinyTag>
              </td>
              <td>{{ scheduleText(item) }}</td>
              <td>{{ displayValue(item.timezone) }}</td>
              <td>{{ displayValue(item.scheduled_at) }}</td>
              <td class="error-cell">{{ displayValue(item.last_error) }}</td>
              <td>
                <div class="row-actions">
                  <TinyButton
                    :data-testid="`pause-schedule-${item.id}`"
                    type="default"
                    :disabled="actingId === item.id || item.status === 'paused'"
                    @click="pauseSchedule(item.id)"
                  >
                    暂停
                  </TinyButton>
                  <TinyButton
                    :data-testid="`trigger-schedule-${item.id}`"
                    type="default"
                    :disabled="actingId === item.id"
                    @click="triggerSchedule(item.id)"
                  >
                    立即触发
                  </TinyButton>
                  <TinyButton
                    :data-testid="`delete-schedule-${item.id}`"
                    type="default"
                    :disabled="actingId === item.id"
                    @click="deleteSchedule(item.id)"
                  >
                    删除
                  </TinyButton>
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
.form-actions,
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

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.form-grid label {
  display: grid;
  gap: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 590;
}

.form-wide {
  grid-column: 1 / -1;
}

.form-actions {
  justify-content: flex-end;
}

.schedule-table-wrap {
  overflow-x: auto;
}

.schedule-table {
  width: 100%;
  border-collapse: collapse;
  min-width: 1120px;
}

.schedule-table th,
.schedule-table td {
  padding: var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
  text-align: left;
  vertical-align: top;
  font-size: var(--text-sm);
}

.schedule-table th {
  color: var(--text-muted);
  font-size: var(--text-xs);
  font-weight: 590;
}

.mono {
  display: block;
  max-width: 220px;
  overflow: hidden;
  color: var(--text-muted);
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.error-cell {
  max-width: 220px;
  color: var(--status-danger);
}

.row-actions {
  align-items: flex-start;
  flex-wrap: wrap;
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
  .ops-grid,
  .form-grid {
    grid-template-columns: 1fr;
  }

  .form-wide {
    grid-column: auto;
  }
}
</style>
