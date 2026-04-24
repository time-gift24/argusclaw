<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";

import { getApiClient, type AgentRecord, type LlmProviderRecord } from "@/lib/api";
import { TinyButton, TinyInput, TinyNumeric, TinyOption, TinySelect, TinySwitch } from "@/lib/opentiny";

const api = getApiClient();
const route = useRoute();
const router = useRouter();

const isEdit = ref(false);

const templates = ref<AgentRecord[]>([]);
const providers = ref<LlmProviderRecord[]>([]);
const loading = ref(true);
const error = ref("");
const saving = ref(false);

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

const templateForm = ref<TemplateFormState>(createDefaultTemplateForm());

const selectedProviderModels = computed(() => {
  const providerId = Number(templateForm.value.provider_id);
  const provider = providers.value.find((item) => item.id === providerId);
  return provider?.models ?? [];
});

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

async function loadData() {
  isEdit.value = !!route.params.templateId;
  if (!isEdit.value) {
    loading.value = false;
    selectDefaultProvider();
    return;
  }

  loading.value = true;
  error.value = "";

  try {
    const providersResult = await api.listProviders();
    providers.value = providersResult;

    const templateId = parseInt(route.params.templateId as string, 10);
    const templatesResult = await api.listTemplates();
    const found = templatesResult.find(t => t.id === templateId);
    if (found) {
      templateForm.value = {
        display_name: found.display_name,
        description: found.description,
        version: found.version,
        provider_id: found.provider_id ? String(found.provider_id) : "",
        model_id: found.model_id ?? "",
        system_prompt: found.system_prompt,
        tool_names: found.tool_names.join("\n"),
        subagent_names: found.subagent_names.join("\n"),
        max_tokens: found.max_tokens ?? null,
        temperature: found.temperature ?? null,
        thinking_enabled: found.thinking_config?.type === "enabled",
        clear_thinking: found.thinking_config?.clear_thinking ?? true,
      };
    } else {
      error.value = "未找到该模板。";
    }
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载数据失败。";
  } finally {
    loading.value = false;
  }
}

async function saveTemplate() {
  if (saving.value) return;

  const payload = buildTemplatePayload();
  if (!payload) return;

  saving.value = true;
  error.value = "";

  try {
    await api.saveTemplate(payload);
    router.push("/templates");
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "保存模板失败。";
  } finally {
    saving.value = false;
  }
}

function buildTemplatePayload(): AgentRecord | null {
  const displayName = templateForm.value.display_name.trim();
  const systemPrompt = templateForm.value.system_prompt.trim();

  if (!displayName || !systemPrompt) {
    error.value = "请填写模板名称和系统提示词。";
    return null;
  }

  return {
    id: isEdit.value ? parseInt(route.params.templateId as string, 10) : 0,
    display_name: displayName,
    description: templateForm.value.description.trim(),
    version: templateForm.value.version.trim() || "1.0.0",
    provider_id: parseProviderId(templateForm.value.provider_id),
    model_id: templateForm.value.model_id.trim() || null,
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
  if (!value) return null;
  const providerId = Number(value);
  return Number.isFinite(providerId) ? providerId : null;
}

function parseList(value: string) {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function goBack() {
  router.push("/templates");
}

onMounted(() => {
  void loadData();
});

watch(
  () => route.params.templateId,
  () => {
    void loadData();
  }
);
</script>

<template>
  <div class="edit-page">
    <article class="template-form-card">
      <div class="form-heading">
        <div>
          <h4>{{ isEdit ? '编辑智能体模板' : '新增智能体模板' }}</h4>
          <p>{{ isEdit ? '修改现有智能体的配置' : '创建 Web 对话可选择的 Agent 配置' }}</p>
        </div>
      </div>

      <p v-if="loading" class="loading-message">加载中...</p>

      <template v-else>
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
            data-testid="save-template"
            type="primary"
            :disabled="saving"
            @click="saveTemplate"
          >
            {{ saving ? "保存中" : (isEdit ? "更新模板" : "创建模板") }}
          </TinyButton>
          <TinyButton type="default" @click="goBack">取消</TinyButton>
        </div>

        <p v-if="error" class="error-message">{{ error }}</p>
      </template>
    </article>
  </div>
</template>

<style scoped>
.edit-page {
  max-width: 1000px;
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
  margin-bottom: var(--space-2);
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
  margin-top: var(--space-2);
}

.switch-row label {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.loading-message {
  text-align: center;
  color: var(--text-muted);
  padding: var(--space-5);
}

.error-message {
  margin-top: var(--space-2);
  padding: var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  background: var(--danger-bg);
  border: 1px solid var(--danger-border);
  color: var(--danger);
}

@media (max-width: 960px) {
  .template-form-grid {
    grid-template-columns: 1fr;
  }
}
</style>
