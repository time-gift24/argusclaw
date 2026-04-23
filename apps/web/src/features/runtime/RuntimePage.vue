<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import { getApiClient, type JobRuntimeSummary, type RuntimeEventSubscription, type RuntimeStateResponse, type ThreadPoolRuntimeSummary } from "@/lib/api";
import { TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const runtime = ref<RuntimeStateResponse | null>(null);
const loading = ref(true);
const error = ref("");
const eventStatus = ref<"connecting" | "connected" | "fallback">("connecting");

let refreshTimer: number | undefined;
let runtimeEvents: RuntimeEventSubscription | undefined;

const threadPoolSnapshot = computed(() => runtime.value?.thread_pool.snapshot ?? null);
const jobRuntimeSnapshot = computed(() => runtime.value?.job_runtime.snapshot ?? null);
const threadPoolRuntimes = computed(() => runtime.value?.thread_pool.runtimes ?? []);
const jobRuntimes = computed(() => runtime.value?.job_runtime.runtimes ?? []);

async function refreshRuntime() {
  try {
    runtime.value = await api.getRuntimeState();
    error.value = "";
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载运行状态失败。";
  } finally {
    loading.value = false;
  }
}

function startPolling() {
  if (refreshTimer) {
    return;
  }

  refreshTimer = window.setInterval(() => {
    void refreshRuntime();
  }, 5000);
}

function connectRuntimeEvents() {
  if (!api.subscribeRuntimeState) {
    eventStatus.value = "fallback";
    startPolling();
    return;
  }

  runtimeEvents = api.subscribeRuntimeState({
    onSnapshot(snapshot) {
      runtime.value = snapshot;
      loading.value = false;
      error.value = "";
      eventStatus.value = "connected";
      if (refreshTimer) {
        window.clearInterval(refreshTimer);
        refreshTimer = undefined;
      }
    },
    onError(reason) {
      eventStatus.value = "fallback";
      error.value = reason.message;
      startPolling();
    },
  });
}

function formatBytes(value: number | null): string {
  if (value == null) {
    return "未知";
  }

  if (value < 1024) {
    return `${value} B`;
  }

  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }

  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

function statusLabel(status: string): string {
  return (
    {
      inactive: "空闲",
      loading: "加载中",
      queued: "排队中",
      running: "运行中",
      cooling: "冷却中",
      evicted: "已逐出",
    }[status] ?? status
  );
}

function reasonLabel(reason: string | null): string {
  if (!reason) {
    return "无";
  }

  return (
    {
      cooling_expired: "冷却到期",
      memory_pressure: "内存压力",
      cancelled: "已取消",
      execution_failed: "执行失败",
    }[reason] ?? reason
  );
}

function statusTagType(status: string): "success" | "info" | "warning" | "danger" {
  if (status === "running") {
    return "success";
  }
  if (status === "queued" || status === "loading" || status === "cooling") {
    return "warning";
  }
  if (status === "evicted") {
    return "danger";
  }

  return "info";
}

onMounted(async () => {
  await refreshRuntime();
  connectRuntimeEvents();
});

onBeforeUnmount(() => {
  runtimeEvents?.close();
  if (refreshTimer) {
    window.clearInterval(refreshTimer);
  }
});
</script>

<template>
  <section class="page-section">
    <div class="runtime-banner">
      <div>
        <h3 class="section-title">运行时总览</h3>
        <p class="section-copy runtime-copy">
          展示线程池与后台 job runtime 的当前负载，优先使用事件流实时更新，断开后自动降级轮询。
        </p>
      </div>
      <TinyTag :type="error ? 'danger' : loading ? 'info' : 'success'">
        {{ error ? "轮询降级" : loading ? "加载中" : eventStatus === "connected" ? "事件流已连接" : "连接事件流" }}
      </TinyTag>
    </div>

    <p
      v-if="error"
      class="error-message"
    >
      {{ error }}
    </p>

    <div
      v-if="threadPoolSnapshot && jobRuntimeSnapshot"
      class="metrics-grid"
    >
      <article class="metric-card">
        <span class="metric-label">线程池活跃数</span>
        <strong class="metric-value">{{ threadPoolSnapshot.active_threads }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">线程池运行中</span>
        <strong class="metric-value">{{ threadPoolSnapshot.running_threads }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">线程池排队数</span>
        <strong class="metric-value">{{ threadPoolSnapshot.queued_threads }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">Job 活跃数</span>
        <strong class="metric-value">{{ jobRuntimeSnapshot.active_threads }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">Job 运行中</span>
        <strong class="metric-value">{{ jobRuntimeSnapshot.running_threads }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">估算内存</span>
        <strong class="metric-value">{{ formatBytes(threadPoolSnapshot.estimated_memory_bytes) }}</strong>
      </article>
    </div>

    <div class="runtime-panels">
      <article class="runtime-panel">
        <div class="panel-header">
          <div>
            <h3 class="panel-title">线程池运行时</h3>
            <p class="panel-description">当前驻留在线程池里的 runtime 记录。</p>
          </div>
          <TinyTag type="info">
            {{ threadPoolRuntimes.length }} 条
          </TinyTag>
        </div>

        <div
          v-if="threadPoolRuntimes.length === 0"
          class="empty-state"
        >
          暂无线程池运行时
        </div>

        <div
          v-else
          class="runtime-list"
        >
          <div
            v-for="runtimeEntry in threadPoolRuntimes"
            :key="runtimeEntry.thread_id"
            class="runtime-row"
          >
            <div class="runtime-main">
              <strong class="runtime-id">{{ runtimeEntry.thread_id }}</strong>
              <span class="runtime-meta">
                Session: {{ runtimeEntry.session_id ?? "未绑定" }}
              </span>
              <span class="runtime-meta">
                最后活跃：{{ runtimeEntry.last_active_at ?? "未知" }}
              </span>
            </div>
            <div class="runtime-side">
              <TinyTag :type="statusTagType(runtimeEntry.status)">
                {{ statusLabel(runtimeEntry.status) }}
              </TinyTag>
              <span class="runtime-memory">{{ formatBytes(runtimeEntry.estimated_memory_bytes) }}</span>
            </div>
          </div>
        </div>
      </article>

      <article class="runtime-panel">
        <div class="panel-header">
          <div>
            <h3 class="panel-title">后台 Job 运行时</h3>
            <p class="panel-description">由后台 job 调度持有的执行 runtime 摘要。</p>
          </div>
          <TinyTag type="info">
            {{ jobRuntimes.length }} 条
          </TinyTag>
        </div>

        <div
          v-if="jobRuntimes.length === 0"
          class="empty-state"
        >
          暂无后台 job runtime
        </div>

        <div
          v-else
          class="runtime-list"
        >
          <div
            v-for="runtimeEntry in jobRuntimes"
            :key="runtimeEntry.job_id"
            class="runtime-row"
          >
            <div class="runtime-main">
              <strong class="runtime-id">{{ runtimeEntry.job_id }}</strong>
              <span class="runtime-meta">
                Thread: {{ runtimeEntry.thread_id }}
              </span>
              <span class="runtime-meta">
                原因：{{ reasonLabel(runtimeEntry.last_reason) }}
              </span>
            </div>
            <div class="runtime-side">
              <TinyTag :type="statusTagType(runtimeEntry.status)">
                {{ statusLabel(runtimeEntry.status) }}
              </TinyTag>
              <span class="runtime-memory">{{ formatBytes(runtimeEntry.estimated_memory_bytes) }}</span>
            </div>
          </div>
        </div>
      </article>
    </div>
  </section>
</template>

<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.runtime-banner,
.metric-card,
.runtime-panel {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.runtime-banner {
  display: flex;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-5) var(--space-6);
  align-items: flex-start;
}

.section-title {
  margin: 0 0 var(--space-2);
  font-size: var(--text-lg);
  font-weight: 590;
  color: var(--text-primary);
}

.runtime-copy {
  max-width: 60ch;
}

.error-message {
  margin: 0;
  padding: var(--space-4);
  color: var(--danger);
  background: var(--danger-bg);
  border: 1px solid var(--danger-border);
  border-radius: var(--radius-md);
}

.metrics-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--space-4);
}

.metric-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-5);
}

.metric-label {
  font-size: var(--text-xs);
  font-weight: 510;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.metric-value {
  font-size: var(--text-xl);
  font-weight: 590;
  color: var(--text-primary);
}

.runtime-panels {
  display: grid;
  gap: var(--space-5);
}

.runtime-panel {
  display: grid;
  gap: var(--space-4);
  padding: var(--space-5);
}

.panel-header {
  display: flex;
  justify-content: space-between;
  gap: var(--space-4);
  align-items: flex-start;
}

.panel-title {
  margin: 0 0 var(--space-1);
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
}

.panel-description {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.runtime-list {
  display: grid;
  gap: var(--space-3);
}

.runtime-row {
  display: flex;
  justify-content: space-between;
  gap: var(--space-4);
  align-items: flex-start;
  padding: var(--space-4);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  background: var(--app-bg);
}

.runtime-main {
  display: grid;
  gap: var(--space-1);
  min-width: 0;
}

.runtime-id {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
  word-break: break-all;
}

.runtime-meta {
  font-size: var(--text-sm);
  color: var(--text-muted);
  word-break: break-all;
}

.runtime-side {
  display: grid;
  gap: var(--space-2);
  justify-items: end;
}

.runtime-memory {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.empty-state {
  padding: var(--space-5);
  border: 1px dashed var(--border-default);
  border-radius: var(--radius-md);
  color: var(--text-muted);
  text-align: center;
}

@media (max-width: 960px) {
  .metrics-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .runtime-banner,
  .panel-header,
  .runtime-row {
    grid-template-columns: 1fr;
    flex-direction: column;
  }

  .runtime-side {
    justify-items: start;
  }
}
</style>
