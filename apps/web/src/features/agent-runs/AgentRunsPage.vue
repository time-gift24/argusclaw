<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import {
  getApiClient,
  type AgentRecord,
  type AgentRunDetail,
  type AgentRunStatus,
  type AgentRunSummary,
} from "@/lib/api";
import { TinyButton, TinyInput, TinyOption, TinySelect, TinyTag } from "@/lib/opentiny";

const templates = ref<AgentRecord[]>([]);
const selectedAgentId = ref("");
const prompt = ref("");
const loading = ref(true);
const submitting = ref(false);
const refreshing = ref(false);
const error = ref("");
const actionMessage = ref("");
const currentRun = ref<AgentRunSummary | AgentRunDetail | null>(null);

const canSubmit = computed(() => {
  return Boolean(selectedAgentId.value) && prompt.value.trim().length > 0 && !submitting.value;
});

function statusType(status: AgentRunStatus): "success" | "info" | "warning" | "danger" {
  if (status === "completed") {
    return "success";
  }
  if (status === "failed") {
    return "danger";
  }
  if (status === "running") {
    return "warning";
  }

  return "info";
}

function statusLabel(status: AgentRunStatus): string {
  return {
    queued: "排队中",
    running: "运行中",
    completed: "已完成",
    failed: "失败",
  }[status];
}

function selectedTemplateName(agentId: number): string {
  return templates.value.find((template) => template.id === agentId)?.display_name ?? `Agent #${agentId}`;
}

function currentRunPrompt(): string | null {
  return "prompt" in (currentRun.value ?? {}) ? (currentRun.value as AgentRunDetail).prompt : null;
}

function currentRunResult(): string | null {
  return "result" in (currentRun.value ?? {}) ? (currentRun.value as AgentRunDetail).result : null;
}

function currentRunError(): string | null {
  return "error" in (currentRun.value ?? {}) ? (currentRun.value as AgentRunDetail).error : null;
}

async function loadTemplates() {
  loading.value = true;
  error.value = "";

  try {
    templates.value = await getApiClient().listTemplates();
    if (!selectedAgentId.value && templates.value.length > 0) {
      selectedAgentId.value = String(templates.value[0].id);
    }
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载智能体模板失败。";
  } finally {
    loading.value = false;
  }
}

async function createRun() {
  const api = getApiClient();
  if (!api.createAgentRun) {
    error.value = "当前 API 客户端不支持 Agent Run。";
    return;
  }

  const trimmedPrompt = prompt.value.trim();
  if (!selectedAgentId.value || !trimmedPrompt) {
    error.value = "请选择智能体并填写提示词。";
    return;
  }

  submitting.value = true;
  error.value = "";
  actionMessage.value = "";

  try {
    currentRun.value = await api.createAgentRun({
      agent_id: Number(selectedAgentId.value),
      prompt: trimmedPrompt,
    });
    actionMessage.value = "运行已创建，可刷新状态查看执行结果。";
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "创建运行失败。";
  } finally {
    submitting.value = false;
  }
}

async function refreshRun() {
  const api = getApiClient();
  if (!api.getAgentRun) {
    error.value = "当前 API 客户端不支持查询 Agent Run。";
    return;
  }
  if (!currentRun.value) {
    return;
  }

  refreshing.value = true;
  error.value = "";

  try {
    currentRun.value = await api.getAgentRun(currentRun.value.run_id);
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "刷新运行状态失败。";
  } finally {
    refreshing.value = false;
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
        <h3 class="page-title">Agent Runs</h3>
        <TinyTag v-if="!loading" type="success">
          {{ templates.length }} 个可选智能体
        </TinyTag>
      </div>
      <TinyButton
        data-testid="refresh-agent-run-templates"
        type="default"
        :disabled="loading"
        @click="loadTemplates"
      >
        {{ loading ? "刷新中" : "刷新模板" }}
      </TinyButton>
    </div>

    <p class="page-copy">
      用 server-only REST API 触发一次独立运行。这里不复用对话页状态，也不会把普通 chat thread 当作 run。
    </p>

    <p v-if="error" class="error-message">
      {{ error }}
    </p>
    <p v-if="actionMessage" class="success-message">
      {{ actionMessage }}
    </p>

    <div v-if="loading" class="loading-state">
      加载中...
    </div>

    <div v-else class="run-layout">
      <article class="run-card">
        <div class="form-grid">
          <label class="field-label" for="agent-run-template">智能体</label>
          <TinySelect
            id="agent-run-template"
            v-model="selectedAgentId"
            data-testid="agent-run-template"
            class="field-control"
          >
            <TinyOption
              v-for="template in templates"
              :key="template.id"
              :label="`${template.display_name} (#${template.id})`"
              :value="String(template.id)"
            />
          </TinySelect>

          <label class="field-label" for="agent-run-prompt">提示词</label>
          <TinyInput
            id="agent-run-prompt"
            v-model="prompt"
            data-testid="agent-run-prompt"
            type="textarea"
            class="prompt-input"
            placeholder="输入要交给指定智能体执行的任务..."
          />
        </div>

        <div class="form-actions">
          <TinyButton
            data-testid="create-agent-run"
            type="primary"
            :disabled="!canSubmit"
            @click="createRun"
          >
            {{ submitting ? "启动中" : "启动运行" }}
          </TinyButton>
        </div>
      </article>

      <article class="run-card run-result">
        <div class="result-header">
          <div>
            <span class="eyebrow">最近一次运行</span>
            <h4 class="result-title">
              {{ currentRun ? selectedTemplateName(currentRun.agent_id) : "尚未创建运行" }}
            </h4>
          </div>
          <TinyTag
            v-if="currentRun"
            :type="statusType(currentRun.status)"
          >
            {{ statusLabel(currentRun.status) }}
          </TinyTag>
        </div>

        <div v-if="!currentRun" class="empty-state">
          选择智能体并提交提示词后，这里会显示 run_id、状态和结果。
        </div>

        <div v-else class="run-detail">
          <dl class="detail-grid">
            <div>
              <dt>Run ID</dt>
              <dd class="mono">{{ currentRun.run_id }}</dd>
            </div>
            <div>
              <dt>Agent ID</dt>
              <dd>{{ currentRun.agent_id }}</dd>
            </div>
            <div>
              <dt>状态值</dt>
              <dd class="mono">{{ currentRun.status }}</dd>
            </div>
            <div>
              <dt>更新时间</dt>
              <dd>{{ currentRun.updated_at }}</dd>
            </div>
          </dl>

          <div v-if="currentRunPrompt()" class="detail-block">
            <span class="detail-label">提示词</span>
            <p>{{ currentRunPrompt() }}</p>
          </div>

          <div v-if="currentRunResult()" class="detail-block">
            <span class="detail-label">结果</span>
            <pre>{{ currentRunResult() }}</pre>
          </div>

          <div v-if="currentRunError()" class="detail-block error-block">
            <span class="detail-label">错误</span>
            <p>{{ currentRunError() }}</p>
          </div>

          <TinyButton
            data-testid="refresh-agent-run"
            type="default"
            :disabled="refreshing"
            @click="refreshRun"
          >
            {{ refreshing ? "刷新中" : "刷新状态" }}
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

.page-header,
.page-header-left,
.result-header,
.form-actions {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.page-header,
.result-header {
  justify-content: space-between;
}

.page-title,
.result-title {
  margin: 0;
  color: var(--text-primary);
}

.page-title {
  font-size: var(--text-base);
  font-weight: 590;
}

.page-copy {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--text-sm);
  line-height: 1.6;
}

.run-layout {
  display: grid;
  grid-template-columns: minmax(320px, 0.9fr) minmax(360px, 1.1fr);
  gap: var(--space-4);
}

.run-card,
.loading-state,
.empty-state {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.run-card {
  display: grid;
  gap: var(--space-5);
  align-content: start;
  padding: var(--space-5);
}

.form-grid,
.run-detail {
  display: grid;
  gap: var(--space-3);
}

.field-label,
.detail-label,
.eyebrow {
  font-size: var(--text-xs);
  font-weight: 590;
  color: var(--text-muted);
}

.field-control {
  width: 100%;
}

.prompt-input :deep(textarea) {
  min-height: 180px;
}

.form-actions {
  justify-content: flex-end;
}

.result-title {
  margin-top: var(--space-1);
  font-size: var(--text-lg);
  font-weight: 590;
}

.detail-grid {
  display: grid;
  grid-template-columns: 1fr 0.4fr 1fr;
  gap: var(--space-3);
  margin: 0;
}

.detail-grid div,
.detail-block {
  padding: var(--space-3);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
}

.detail-grid dt {
  color: var(--text-muted);
  font-size: var(--text-xs);
}

.detail-grid dd,
.detail-block p {
  margin: var(--space-1) 0 0;
  color: var(--text-primary);
  font-size: var(--text-sm);
}

.mono,
.detail-block pre {
  font-family: var(--font-mono);
}

.detail-block pre {
  overflow: auto;
  max-height: 320px;
  margin: var(--space-2) 0 0;
  white-space: pre-wrap;
  color: var(--text-primary);
  font-size: var(--text-sm);
}

.error-block {
  border-color: var(--danger-border);
  background: var(--danger-bg);
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
  padding: var(--space-8) var(--space-4);
  color: var(--text-muted);
  text-align: center;
}

@media (max-width: 960px) {
  .run-layout,
  .detail-grid {
    grid-template-columns: 1fr;
  }
}
</style>
