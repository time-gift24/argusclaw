<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";

import { getApiClient, type AgentRecord } from "@/lib/api";
import { TinyButton, TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const router = useRouter();

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

function goToCreate() {
  router.push("/templates/new");
}

function editTemplate(template: AgentRecord) {
  router.push(`/templates/${template.id}/edit`);
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
        <TinyTag v-if="!loading" type="success">
          {{ templates.length }} 项
        </TinyTag>
      </div>
      <div class="page-header-right">
        <TinyButton type="primary" @click="goToCreate">
          新增模板
        </TinyButton>
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
      <TinyButton type="primary" @click="goToCreate">立即创建</TinyButton>
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
          <div class="template-title-group">
            <strong class="template-name">{{ template.display_name }}</strong>
            <span class="template-version">v{{ template.version }}</span>
          </div>
        </div>
        <p class="template-description">{{ template.description }}</p>
        <div class="template-actions">
          <TinyButton
            :data-testid="`edit-template-${template.id}`"
            type="default"
            @click="editTemplate(template)"
          >
            编辑
          </TinyButton>
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
  gap: var(--space-4);
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
  grid-template-columns: repeat(auto-fill, minmax(400px, 1fr));
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
  transition: border-color var(--transition-base);
}

.template-card:hover {
  border-color: var(--border-strong);
}

.template-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.template-title-group {
  display: flex;
  align-items: center;
  gap: var(--space-2);
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
  flex: 1;
}

.template-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-2);
  border-top: 1px solid var(--border-subtle);
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
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-4);
}

@media (max-width: 960px) {
  .template-grid {
    grid-template-columns: 1fr;
  }
}
</style>
