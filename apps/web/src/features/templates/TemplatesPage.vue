<script setup lang="ts">
import { onMounted, ref } from "vue";

import { getApiClient, type AgentRecord } from "@/lib/api";
import { TinyButton, TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const templates = ref<AgentRecord[]>([]);
const loading = ref(true);
const error = ref("");
const actionMessage = ref("");
const deletingTemplateId = ref<number | null>(null);

async function loadTemplates() {
  loading.value = true;
  error.value = "";

  try {
    templates.value = await api.listTemplates();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载模板失败。";
  } finally {
    loading.value = false;
  }
}

async function deleteTemplate(template: AgentRecord) {
  if (!api.deleteTemplate) {
    error.value = "当前 API 客户端不支持删除模板。";
    return;
  }

  deletingTemplateId.value = template.id;
  error.value = "";
  actionMessage.value = "";

  try {
    await api.deleteTemplate(template.id);
    actionMessage.value = "模板已删除。";
    await loadTemplates();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "删除模板失败。";
  } finally {
    deletingTemplateId.value = null;
  }
}

onMounted(() => {
  void loadTemplates();
});
</script>

<template>
  <section class="page-section">
    <div class="page-header">
      <div class="page-header-left">
        <h3 class="page-title">智能体模板</h3>
        <TinyTag v-if="!loading">
          {{ templates.length }} 项
        </TinyTag>
      </div>
    </div>

    <div
      v-if="loading"
      class="loading-state"
    >
      加载中...
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
      v-if="!loading && templates.length === 0"
      class="empty-state"
    >
      <p>暂无可用的模板</p>
    </div>

    <div
      v-if="!loading && templates.length > 0"
      class="template-grid"
    >
      <article
        v-for="template in templates"
        :key="template.id"
        class="template-card"
      >
        <div class="template-header">
          <strong class="template-name">{{ template.display_name }}</strong>
          <span class="template-version">v{{ template.version }}</span>
        </div>
        <p class="template-description">{{ template.description }}</p>
        <div class="template-actions">
          <TinyButton
            :data-testid="`delete-template-${template.id}`"
            type="default"
            :disabled="deletingTemplateId === template.id"
            @click="deleteTemplate(template)"
          >
            {{ deletingTemplateId === template.id ? "删除中" : "删除模板" }}
          </TinyButton>
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

.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.page-header-left {
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

.template-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.template-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-5);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  transition:
    border-color var(--transition-base),
    transform var(--transition-fast);
}

.template-card:hover {
  border-color: var(--border-strong);
}

.template-card:active {
  transform: scale(0.99);
}

.template-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.template-name {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
}

.template-version {
  font-size: var(--text-xs);
  font-weight: 510;
  color: var(--text-muted);
  padding: var(--space-1) var(--space-2);
  background: var(--surface-raised);
  border-radius: var(--radius-sm);
}

.template-description {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
  line-height: 1.5;
}

.template-actions {
  display: flex;
  justify-content: flex-end;
  padding-top: var(--space-2);
}

.error-message,
.success-message {
  margin: 0;
  padding: var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}

.error-message {
  background: var(--danger-bg);
  border: 1px solid var(--danger-border);
  color: var(--danger);
}

.success-message {
  background: var(--success-bg);
  border: 1px solid var(--success-border);
  color: var(--success);
}

.loading-state,
.empty-state {
  padding: var(--space-10) var(--space-4);
  text-align: center;
  color: var(--text-muted);
  font-size: var(--text-sm);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
}

@media (max-width: 960px) {
  .template-grid {
    grid-template-columns: 1fr;
  }
}
</style>
