<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import { getApiClient, type RiskLevel, type ToolRegistryItem } from "@/lib/api";
import { TinyButton, TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const tools = ref<ToolRegistryItem[]>([]);
const loading = ref(true);
const error = ref("");

const summary = computed(() => {
  const riskyTools = tools.value.filter((tool) => tool.risk_level === "high" || tool.risk_level === "critical");

  return {
    total: tools.value.length,
    highOrAbove: riskyTools.length,
    critical: tools.value.filter((tool) => tool.risk_level === "critical").length,
    medium: tools.value.filter((tool) => tool.risk_level === "medium").length,
  };
});

function riskType(level: RiskLevel): "success" | "info" | "warning" | "danger" {
  if (level === "critical" || level === "high") {
    return "danger";
  }
  if (level === "medium") {
    return "warning";
  }

  return "success";
}

function schemaPreview(tool: ToolRegistryItem) {
  return JSON.stringify(tool.parameters, null, 2);
}

async function loadTools() {
  loading.value = true;
  error.value = "";

  if (!api.listTools) {
    error.value = "当前 API 客户端不支持工具注册表。";
    loading.value = false;
    return;
  }

  try {
    tools.value = await api.listTools();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载工具注册表失败。";
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  void loadTools();
});
</script>

<template>
  <section class="page-section">
    <div class="page-header">
      <div class="page-header-left">
        <h3 class="page-title">工具注册表</h3>
        <TinyTag v-if="!loading">
          {{ tools.length }} 项
        </TinyTag>
      </div>
      <TinyButton
        data-testid="refresh-tools"
        type="default"
        :disabled="loading"
        @click="loadTools"
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
        <span class="ops-label">总工具</span>
        <strong class="ops-value">{{ summary.total }}</strong>
      </article>
      <article class="ops-card">
        <span class="ops-label">高风险及以上</span>
        <strong class="ops-value">{{ summary.highOrAbove }}</strong>
      </article>
      <article class="ops-card">
        <span class="ops-label">Critical</span>
        <strong class="ops-value">{{ summary.critical }}</strong>
      </article>
      <article class="ops-card">
        <span class="ops-label">Medium</span>
        <strong class="ops-value">{{ summary.medium }}</strong>
      </article>
    </div>

    <div
      v-if="!loading && tools.length === 0"
      class="empty-state"
    >
      暂无已注册工具
    </div>

    <div
      v-if="!loading && tools.length > 0"
      class="tool-list"
    >
      <article
        v-for="tool in tools"
        :key="tool.name"
        class="tool-card"
      >
        <div class="tool-main">
          <div class="tool-header">
            <strong class="tool-name">{{ tool.name }}</strong>
            <TinyTag :type="riskType(tool.risk_level)">
              {{ tool.risk_level }}
            </TinyTag>
          </div>
          <p class="tool-description">{{ tool.description || "暂无描述" }}</p>
        </div>
        <details class="schema-panel">
          <summary>参数 Schema</summary>
          <pre>{{ schemaPreview(tool) }}</pre>
        </details>
      </article>
    </div>
  </section>
</template>

<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
}

.page-header-left,
.tool-header {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.page-title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
}

.ops-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--space-3);
}

.ops-card,
.tool-card,
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

.tool-list {
  display: grid;
  gap: var(--space-3);
}

.tool-card {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-4) var(--space-5);
}

.tool-main {
  display: grid;
  gap: var(--space-2);
}

.tool-name {
  font-size: var(--text-sm);
  color: var(--text-primary);
}

.tool-description {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--text-sm);
  line-height: 1.5;
}

.schema-panel {
  font-size: var(--text-xs);
}

.schema-panel summary {
  cursor: pointer;
  color: var(--accent);
}

.schema-panel pre {
  overflow: auto;
  margin: var(--space-2) 0 0;
  padding: var(--space-3);
  max-height: 320px;
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
}

.loading-state,
.empty-state {
  padding: var(--space-10) var(--space-4);
  text-align: center;
  color: var(--text-muted);
  font-size: var(--text-sm);
}

.error-message {
  margin: 0;
  padding: var(--space-3);
  background: var(--danger-bg);
  border: 1px solid var(--danger-border);
  border-radius: var(--radius-md);
  color: var(--danger);
  font-size: var(--text-sm);
}

@media (max-width: 960px) {
  .ops-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
