<script setup lang="ts">
import { onMounted, ref } from "vue";

import { getApiClient, type BootstrapResponse } from "@/lib/api";
import { TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const bootstrap = ref<BootstrapResponse | null>(null);

onMounted(async () => {
  bootstrap.value = await api.getBootstrap();
});
</script>

<template>
  <section class="page-section">
    <div
      v-if="bootstrap"
      class="metrics-grid"
    >
      <article class="metric-card">
        <span class="metric-label">实例名称</span>
        <strong class="metric-value">{{ bootstrap.instance_name }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">模型提供方</span>
        <strong class="metric-value">{{ bootstrap.provider_count }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">模板数量</span>
        <strong class="metric-value">{{ bootstrap.template_count }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">MCP 服务</span>
        <strong class="metric-value">{{ bootstrap.mcp_server_count }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">MCP 就绪</span>
        <strong class="metric-value">{{ bootstrap.mcp_ready_count }}</strong>
      </article>
      <article class="metric-card">
        <span class="metric-label">默认提供方</span>
        <strong class="metric-value">{{ bootstrap.default_provider_id ?? "未设置" }}</strong>
      </article>
    </div>
  </section>
</template>

<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
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
  font-weight: 510;
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
