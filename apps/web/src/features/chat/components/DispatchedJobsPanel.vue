<script setup lang="ts">
import type { ChatThreadJobSummary } from "@/lib/api";

const props = defineProps<{
  jobs: ChatThreadJobSummary[];
  loading: boolean;
  error: string;
}>();

const emit = defineEmits<{
  openJob: [jobId: string];
  refresh: [];
}>();

function statusText(status: string) {
  switch (status) {
    case "pending":
      return "待派发";
    case "queued":
      return "排队中";
    case "running":
      return "运行中";
    case "succeeded":
      return "已完成";
    case "failed":
      return "失败";
    case "cancelled":
      return "已取消";
    default:
      return status || "未知";
  }
}

function openJob(jobId: string) {
  emit("openJob", jobId);
}
</script>

<template>
  <aside class="dispatched-jobs" aria-label="已派发 subagent">
    <header class="dispatched-jobs__header">
      <div class="dispatched-jobs__summary">
        <p>已派发 subagent</p>
        <span>{{ props.jobs.length }}</span>
      </div>
      <button
        type="button"
        class="dispatched-jobs__refresh"
        title="刷新"
        :disabled="props.loading"
        @click="emit('refresh')"
      >
        刷新
      </button>
    </header>

    <p v-if="props.loading" class="dispatched-jobs__state">正在加载派发记录...</p>
    <p v-else-if="props.error" class="dispatched-jobs__state dispatched-jobs__state--error">
      派发记录加载失败，可刷新重试
    </p>
    <p v-else-if="props.jobs.length === 0" class="dispatched-jobs__state">暂无派发的 subagent</p>

    <div v-else class="dispatched-jobs__list">
      <button
        v-for="job in props.jobs"
        :key="job.job_id"
        type="button"
        class="dispatched-jobs__row"
        :data-testid="`dispatched-job-${job.job_id}`"
        @click="openJob(job.job_id)"
      >
        <span class="dispatched-jobs__body">
          <strong>{{ job.title || job.job_id }}</strong>
          <small>{{ job.subagent_name }}</small>
          <em v-if="job.result_preview">{{ job.result_preview }}</em>
        </span>
        <span class="dispatched-jobs__status" :class="`dispatched-jobs__status--${job.status}`">
          {{ statusText(job.status) }}
        </span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.dispatched-jobs {
  display: grid;
  gap: var(--space-3);
  width: 100%;
  max-height: 300px;
  overflow: hidden;
  padding: var(--space-4);
  border: 1px solid color-mix(in srgb, var(--border-default) 78%, transparent);
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--surface-base) 94%, transparent);
  box-shadow: var(--shadow-xs);
}

.dispatched-jobs__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}

.dispatched-jobs__summary {
  display: inline-flex;
  min-width: 0;
  align-items: center;
  gap: var(--space-2);
}

.dispatched-jobs__summary p {
  margin: 0;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  white-space: nowrap;
}

.dispatched-jobs__summary span {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 22px;
  height: 22px;
  padding: 0 var(--space-2);
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 12%, white);
  color: var(--accent);
  font-size: var(--text-xs);
  font-weight: 700;
}

.dispatched-jobs__refresh {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  min-width: 48px;
  height: 28px;
  padding: 0 var(--space-2);
  border: 1px solid color-mix(in srgb, var(--border-default) 80%, transparent);
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--surface-base) 86%, transparent);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 700;
  cursor: pointer;
}

.dispatched-jobs__refresh:disabled {
  cursor: wait;
  opacity: 0.62;
}

.dispatched-jobs__refresh:not(:disabled):hover {
  border-color: color-mix(in srgb, var(--accent) 38%, var(--border-default));
  background: color-mix(in srgb, var(--accent) 9%, transparent);
  color: var(--accent);
}

.dispatched-jobs__state {
  margin: 0;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  line-height: 1.6;
}

.dispatched-jobs__state--error {
  color: var(--status-danger);
}

.dispatched-jobs__list {
  display: grid;
  min-height: 0;
  overflow: auto;
  gap: var(--space-2);
  padding-right: 2px;
}

.dispatched-jobs__row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-3);
  border: 1px solid color-mix(in srgb, var(--border-default) 75%, transparent);
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--surface-base) 88%, transparent);
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.dispatched-jobs__row:hover {
  border-color: color-mix(in srgb, var(--accent) 38%, var(--border-default));
  background: color-mix(in srgb, var(--accent) 7%, transparent);
}

.dispatched-jobs__body {
  display: grid;
  min-width: 0;
  gap: 2px;
}

.dispatched-jobs__body strong,
.dispatched-jobs__body small,
.dispatched-jobs__body em {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dispatched-jobs__body strong {
  color: var(--text-primary);
  font-size: var(--text-sm);
  line-height: 1.45;
}

.dispatched-jobs__body small,
.dispatched-jobs__body em {
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-style: normal;
  line-height: 1.45;
}

.dispatched-jobs__status {
  align-self: start;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 3px 9px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--surface-muted) 92%, white);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 650;
  line-height: 1.4;
  white-space: nowrap;
}

.dispatched-jobs__status--running,
.dispatched-jobs__status--queued,
.dispatched-jobs__status--pending {
  background: color-mix(in srgb, var(--accent) 12%, white);
  color: var(--accent);
}

.dispatched-jobs__status--succeeded {
  background: color-mix(in srgb, var(--status-success) 12%, white);
  color: var(--status-success);
}

.dispatched-jobs__status--failed,
.dispatched-jobs__status--cancelled {
  background: color-mix(in srgb, var(--status-danger) 12%, white);
  color: var(--status-danger);
}
</style>
