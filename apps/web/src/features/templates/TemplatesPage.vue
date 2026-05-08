<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";

import { getApiClient, type AgentDeleteReport, type AgentRecord } from "@/lib/api";
import { TinyButton, TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const router = useRouter();

const templates = ref<AgentRecord[]>([]);
const loading = ref(true);
const error = ref("");
const actionMessage = ref("");
const deletingTemplateId = ref<number | null>(null);
const cascadeConfirmTemplate = ref<AgentRecord | null>(null);

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

function isReferenceBlockedError(reason: unknown): boolean {
  if (!(reason instanceof Error)) {
    return false;
  }

  return reason.message.includes("无法删除智能体") && reason.message.includes("引用");
}

function formatDeleteMessage(report: AgentDeleteReport): string {
  const details = [
    report.deleted_job_count > 0 ? `${report.deleted_job_count} 个任务` : "",
    report.deleted_run_count > 0 ? `${report.deleted_run_count} 条运行记录` : "",
    report.deleted_thread_count > 0 ? `${report.deleted_thread_count} 个线程` : "",
    report.deleted_session_count > 0 ? `${report.deleted_session_count} 个空会话` : "",
  ].filter(Boolean);

  return details.length > 0
    ? `模板及关联的 ${details.join("、")} 已删除。`
    : "模板已删除。";
}

async function deleteTemplate(template: AgentRecord) {
  if (!api.deleteTemplate) {
    error.value = "当前 API 客户端不支持删除模板。";
    return;
  }

  deletingTemplateId.value = template.id;
  error.value = "";
  actionMessage.value = "";
  cascadeConfirmTemplate.value = null;

  try {
    const report = await api.deleteTemplate(template.id);
    actionMessage.value = formatDeleteMessage(report);
    await loadTemplates();
  } catch (reason) {
    if (isReferenceBlockedError(reason)) {
      cascadeConfirmTemplate.value = template;
      error.value = "";
    } else {
      error.value = reason instanceof Error ? reason.message : "删除模板失败。";
    }
  } finally {
    deletingTemplateId.value = null;
  }
}

async function confirmCascadeDelete() {
  if (!api.deleteTemplate || !cascadeConfirmTemplate.value) {
    return;
  }

  const template = cascadeConfirmTemplate.value;
  deletingTemplateId.value = template.id;
  error.value = "";
  actionMessage.value = "";

  try {
    const report = await api.deleteTemplate(template.id, { cascadeAssociations: true });
    cascadeConfirmTemplate.value = null;
    actionMessage.value = formatDeleteMessage(report);
    await loadTemplates();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "删除模板失败。";
  } finally {
    deletingTemplateId.value = null;
  }
}

function cancelCascadeDelete() {
  cascadeConfirmTemplate.value = null;
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
      v-if="cascadeConfirmTemplate"
      class="cascade-confirm"
      data-testid="cascade-delete-confirmation"
    >
      <div>
        <strong>{{ cascadeConfirmTemplate.display_name }}</strong>
        <p>
          该智能体仍有关联任务或会话线程。继续删除会同步清理关联任务、匹配线程，以及清理后为空的会话。
        </p>
      </div>
      <div class="cascade-confirm-actions">
        <TinyButton
          data-testid="cancel-cascade-delete"
          type="default"
          :disabled="deletingTemplateId === cascadeConfirmTemplate.id"
          @click="cancelCascadeDelete"
        >
          取消
        </TinyButton>
        <TinyButton
          data-testid="confirm-cascade-delete"
          type="danger"
          :disabled="deletingTemplateId === cascadeConfirmTemplate.id"
          @click="confirmCascadeDelete"
        >
          {{
            deletingTemplateId === cascadeConfirmTemplate.id
              ? "删除中"
              : "同时删除关联数据"
          }}
        </TinyButton>
      </div>
    </div>

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

.cascade-confirm {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4);
  background: var(--status-warning-bg);
  border: 1px solid var(--warning-border);
  border-radius: var(--radius-md);
}

.cascade-confirm strong {
  display: block;
  margin-bottom: var(--space-1);
  font-size: var(--text-sm);
  color: var(--text-primary);
}

.cascade-confirm p {
  margin: 0;
  font-size: var(--text-sm);
  line-height: 1.5;
  color: var(--text-secondary);
}

.cascade-confirm-actions {
  display: flex;
  flex-shrink: 0;
  justify-content: flex-end;
  gap: var(--space-2);
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
