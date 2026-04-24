<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import { getApiClient, type AgentRecord, type LlmProviderRecord } from "@/lib/api";
import { TinyButton, TinyInput, TinyNumeric, TinyOption, TinySelect, TinySwitch, TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const templates = ref<AgentRecord[]>([]);
const providers = ref<LlmProviderRecord[]>([]);
const loading = ref(true);
const error = ref("");
const actionMessage = ref("");
const saving = ref(false);
const deletingTemplateId = ref<number | null>(null);
const templateForm = ref(createDefaultTemplateForm());

const selectedProviderModels = computed(() => {
  const providerId = Number(templateForm.value.provider_id);
  const provider = providers.value.find((item) => item.id === providerId);
  return provider?.models ?? [];
});

interface TemplateFormState {
  display_name: string;
  description: string;
  version: string;
  provider_id: string;
  model_id: string;
  system_prompt: string;
  tool_names: string;
  subagent_names: string;
  max_tokens: number | null;
  temperature: number | null;
  thinking_enabled: boolean;
  clear_thinking: boolean;
}

function createDefaultTemplateForm(): TemplateFormState {
  return {
    display_name: "",
    description: "",
    version: "1.0.0",
    provider_id: "",
    model_id: "",
    system_prompt: "",
    tool_names: "",
    subagent_names: "",
    max_tokens: null,
    temperature: null,
    thinking_enabled: false,
    clear_thinking: true,
  };
}

async function loadInitialState() {
  loading.value = true;
  error.value = "";
  const loadErrors: string[] = [];

  const [templatesResult, providersResult] = await Promise.allSettled([
    api.listTemplates(),
    api.listProviders(),
  ]);

  if (templatesResult.status === "fulfilled") {
    templates.value = templatesResult.value;
  } else {
    loadErrors.push(errorMessage(templatesResult.reason, "加载模板失败。"));
  }

  if (providersResult.status === "fulfilled") {
    providers.value = providersResult.value;
    selectDefaultProvider();
  } else {
    loadErrors.push(errorMessage(providersResult.reason, "加载模型提供方失败。"));
  }

  if (loadErrors.length > 0) {
    error.value = loadErrors.join("；");
  }
  loading.value = false;
}

async function loadTemplates() {
  loading.value = true;
  error.value = "";

  try {
    templates.value = await api.listTemplates();
  } catch (reason) {
    error.value = errorMessage(reason, "加载模板失败。");
  } finally {
    loading.value = false;
  }
}

async function createTemplate() {
  if (saving.value) {
    return;
  }

  const payload = buildTemplatePayload();
  if (!payload) {
    return;
  }

  saving.value = true;
  error.value = "";
  actionMessage.value = "";

  try {
    await api.saveTemplate(payload);
    actionMessage.value = "模板已创建。";
    templateForm.value = createDefaultTemplateForm();
    selectDefaultProvider();
    await loadTemplates();
  } catch (reason) {
    error.value = errorMessage(reason, "创建模板失败。");
  } finally {
    saving.value = false;
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

function buildTemplatePayload(): AgentRecord | null {
  const displayName = templateForm.value.display_name.trim();
  const systemPrompt = templateForm.value.system_prompt.trim();

  if (!displayName || !systemPrompt) {
    error.value = "请填写模板名称和系统提示词。";
    actionMessage.value = "";
    return null;
  }

  return {
    id: 0,
    display_name: displayName,
    description: templateForm.value.description.trim(),
    version: templateForm.value.version.trim() || "1.0.0",
    provider_id: parseProviderId(templateForm.value.provider_id),
    model_id: normalizeOptionalText(templateForm.value.model_id),
    system_prompt: systemPrompt,
    tool_names: parseList(templateForm.value.tool_names),
    subagent_names: parseList(templateForm.value.subagent_names),
    max_tokens: templateForm.value.max_tokens,
    temperature: templateForm.value.temperature,
    thinking_config: templateForm.value.thinking_enabled
      ? {
          type: "enabled",
          clear_thinking: templateForm.value.clear_thinking,
        }
      : null,
  };
}

function selectDefaultProvider() {
  const provider = providers.value.find((item) => item.is_default) ?? providers.value[0] ?? null;
  templateForm.value.provider_id = provider ? String(provider.id) : "";
  templateForm.value.model_id = provider?.default_model ?? "";
}

function handleProviderChange(value: string | number) {
  templateForm.value.provider_id = String(value);
  const provider = providers.value.find((item) => item.id === Number(value));
  templateForm.value.model_id = provider?.default_model ?? "";
}

function parseProviderId(value: string) {
  if (!value) {
    return null;
  }

  const providerId = Number(value);
  return Number.isFinite(providerId) ? providerId : null;
}

function normalizeOptionalText(value: string) {
  const nextValue = value.trim();
  return nextValue ? nextValue : null;
}

function parseList(value: string) {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function errorMessage(reason: unknown, fallback: string) {
  return reason instanceof Error ? reason.message : fallback;
}

onMounted(() => {
  void loadInitialState();
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

    <article class="template-form-card">
      <div class="form-heading">
        <div>
          <h4>新增智能体模板</h4>
          <p>创建 Web 对话可选择的 Agent 配置。</p>
        </div>
        <TinyTag type="info">创建</TinyTag>
      </div>

      <div class="template-form-grid">
        <label>
          <span>模板名称</span>
          <TinyInput
            v-model="templateForm.display_name"
            data-testid="template-display-name"
            placeholder="例如：代码助手"
          />
        </label>
        <label>
          <span>版本</span>
          <TinyInput
            v-model="templateForm.version"
            data-testid="template-version"
            placeholder="1.0.0"
          />
        </label>
        <label>
          <span>描述</span>
          <TinyInput
            v-model="templateForm.description"
            data-testid="template-description"
            placeholder="说明这个模板适合什么任务"
          />
        </label>
        <label>
          <span>模型提供方</span>
          <TinySelect
            v-model="templateForm.provider_id"
            data-testid="template-provider"
            @change="handleProviderChange"
          >
            <TinyOption
              label="不绑定提供方"
              value=""
            />
            <TinyOption
              v-for="provider in providers"
              :key="provider.id"
              :label="provider.display_name"
              :value="String(provider.id)"
            />
          </TinySelect>
        </label>
        <label>
          <span>模型</span>
          <TinySelect
            v-if="selectedProviderModels.length > 0"
            v-model="templateForm.model_id"
            data-testid="template-model"
          >
            <TinyOption
              label="不绑定模型"
              value=""
            />
            <TinyOption
              v-for="model in selectedProviderModels"
              :key="model"
              :label="model"
              :value="model"
            />
          </TinySelect>
          <TinyInput
            v-else
            v-model="templateForm.model_id"
            data-testid="template-model"
            placeholder="例如 glm-4.7"
          />
        </label>
        <label>
          <span>最大 Tokens</span>
          <TinyNumeric
            v-model="templateForm.max_tokens"
            data-testid="template-max-tokens"
            placeholder="可选"
          />
        </label>
        <label>
          <span>Temperature</span>
          <TinyNumeric
            v-model="templateForm.temperature"
            data-testid="template-temperature"
            placeholder="可选，例如 0.2"
          />
        </label>
      </div>

      <label class="full-field">
        <span>系统提示词</span>
        <TinyInput
          v-model="templateForm.system_prompt"
          data-testid="template-system-prompt"
          type="textarea"
          :rows="5"
          placeholder="描述智能体身份、边界和工作方式"
        />
      </label>

      <div class="template-form-grid">
        <label>
          <span>工具列表</span>
          <TinyInput
            v-model="templateForm.tool_names"
            data-testid="template-tools"
            type="textarea"
            :rows="3"
            placeholder="每行一个工具，例如 read"
          />
        </label>
        <label>
          <span>子智能体列表</span>
          <TinyInput
            v-model="templateForm.subagent_names"
            data-testid="template-subagents"
            type="textarea"
            :rows="3"
            placeholder="每行一个子智能体"
          />
        </label>
      </div>

      <div class="switch-row">
        <label>
          <TinySwitch
            v-model="templateForm.thinking_enabled"
            data-testid="template-thinking"
          />
          <span>启用 Thinking</span>
        </label>
        <label>
          <TinySwitch v-model="templateForm.clear_thinking" />
          <span>清理 Thinking 内容</span>
        </label>
      </div>

      <div class="form-actions">
        <TinyButton
          data-testid="create-template"
          type="primary"
          :disabled="saving"
          @click="createTemplate"
        >
          {{ saving ? "创建中" : "创建模板" }}
        </TinyButton>
      </div>
    </article>

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

.template-form-card {
  display: grid;
  gap: var(--space-4);
  padding: var(--space-5);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.form-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
}

.form-heading h4 {
  margin: 0;
  color: var(--text-primary);
  font-size: var(--text-base);
  font-weight: 650;
}

.form-heading p {
  margin: var(--space-1) 0 0;
  color: var(--text-muted);
  font-size: var(--text-sm);
}

.template-form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.template-form-grid label,
.full-field {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 590;
}

.switch-row,
.form-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
}

.switch-row label {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-sm);
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
  .template-form-grid,
  .template-grid {
    grid-template-columns: 1fr;
  }
}
</style>
