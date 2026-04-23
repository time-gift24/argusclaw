<script setup lang="ts">
import { onMounted, ref } from "vue";

import { getApiClient, type BootstrapResponse } from "@/lib/api";
import { TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const healthStatus = ref<"loading" | "healthy" | "unhealthy">("loading");
const bootstrap = ref<BootstrapResponse | null>(null);

onMounted(async () => {
  const [health, bootstrapData] = await Promise.all([
    api.getHealth(),
    api.getBootstrap(),
  ]);
  healthStatus.value = health.status === "ok" ? "healthy" : "unhealthy";
  bootstrap.value = bootstrapData;
});
</script>

<template>
  <section class="page-section">
    <div class="status-banner">
      <div class="status-info">
        <div class="status-indicator">
          <span
            class="status-dot"
            :class="healthStatus"
          ></span>
          <span class="status-label">
            {{ healthStatus === 'healthy' ? '健康' : healthStatus === 'unhealthy' ? '异常' : '检查中' }}
          </span>
        </div>
        <div
          v-if="bootstrap"
          class="instance-name"
        >
          {{ bootstrap.instance_name }}
        </div>
      </div>
      <TinyTag
        :type="healthStatus === 'healthy' ? 'success' : 'danger'"
      >
        {{ healthStatus === 'healthy' ? '运行正常' : healthStatus === 'unhealthy' ? '异常' : '检查中' }}
      </TinyTag>
    </div>

    <div
      v-if="bootstrap"
      class="metrics-grid"
    >
      <article class="metric-card">
        <span class="metric-label">模型提供方</span>
        <strong class="metric-value">{{ bootstrap.provider_count }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">模板数量</span>
        <strong class="metric-value">{{ bootstrap.template_count }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">MCP 就绪</span>
        <strong class="metric-value">{{ bootstrap.mcp_ready_count }}</strong>
      </article>
    </div>
  </section>
</template>

<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.status-banner {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-5) var(--space-6);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.status-info {
  display: flex;
  align-items: center;
  gap: var(--space-4);
}

.status-indicator {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.status-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--text-placeholder);
}

.status-dot.healthy {
  background: var(--success);
}

.status-dot.unhealthy {
  background: var(--danger);
}

.status-label {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
}

.instance-name {
  font-size: var(--text-sm);
  color: var(--text-muted);
  padding-left: var(--space-4);
  border-left: 1px solid var(--border-subtle);
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
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.metric-label {
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.metric-value {
  font-size: var(--text-xl);
  font-weight: 590;
  color: var(--text-primary);
  letter-spacing: -0.16px;
}

@media (max-width: 960px) {
  .metrics-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
