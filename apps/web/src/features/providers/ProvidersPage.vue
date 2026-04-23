<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import ProviderForm from "./ProviderForm.vue";
import { getApiClient, type LlmProviderRecord } from "@/lib/api";
import { TinyButton, TinyTag } from "@/lib/opentiny";

const api = getApiClient();

function createDraft(overrides: Partial<LlmProviderRecord> = {}): LlmProviderRecord {
  return {
    id: 0,
    kind: "openai-compatible",
    display_name: "",
    base_url: "https://api.openai.com/v1",
    api_key: "",
    models: ["gpt-4.1"],
    model_config: {},
    default_model: "gpt-4.1",
    is_default: false,
    extra_headers: {},
    secret_status: "ready",
    meta_data: {},
    ...overrides,
  };
}

const providers = ref<LlmProviderRecord[]>([]);
const saving = ref(false);
const loading = ref(true);
const error = ref("");
const actionMessage = ref("");
const testingProviderId = ref<number | null>(null);
const deletingProviderId = ref<number | null>(null);
const draft = ref<LlmProviderRecord>(createDraft());

const submitLabel = computed(() => {
  if (saving.value) {
    return draft.value.id > 0 ? "更新中…" : "保存中…";
  }

  return draft.value.id > 0 ? "更新提供方" : "创建提供方";
});

async function loadProviders() {
  loading.value = true;
  error.value = "";

  try {
    providers.value = await api.listProviders();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载提供方失败。";
  } finally {
    loading.value = false;
  }
}

function resetDraft() {
  draft.value = createDraft();
}

function editProvider(provider: LlmProviderRecord) {
  error.value = "";
  draft.value = createDraft({
    ...provider,
    api_key: "",
  });
}

async function saveProvider() {
  saving.value = true;
  error.value = "";
  actionMessage.value = "";

  try {
    await api.saveProvider(draft.value);
    actionMessage.value = draft.value.id > 0 ? "提供方已更新。" : "提供方已创建。";
    resetDraft();
    await loadProviders();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "保存提供方失败。";
  } finally {
    saving.value = false;
  }
}

async function testProvider(provider: LlmProviderRecord) {
  if (!api.testProvider) {
    error.value = "当前 API 客户端不支持提供方连接测试。";
    return;
  }

  testingProviderId.value = provider.id;
  error.value = "";
  actionMessage.value = "";

  try {
    const result = await api.testProvider(provider.id, provider.default_model);
    actionMessage.value = `连接测试：${result.message}`;
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "连接测试失败。";
  } finally {
    testingProviderId.value = null;
  }
}

async function testDraftProvider() {
  if (!api.testProviderDraft) {
    error.value = "当前 API 客户端不支持临时配置连接测试。";
    return;
  }

  testingProviderId.value = 0;
  error.value = "";
  actionMessage.value = "";

  try {
    const result = await api.testProviderDraft(draft.value);
    actionMessage.value = `当前配置测试：${result.message}`;
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "当前配置测试失败。";
  } finally {
    testingProviderId.value = null;
  }
}

async function deleteProvider(provider: LlmProviderRecord) {
  if (!api.deleteProvider) {
    error.value = "当前 API 客户端不支持删除提供方。";
    return;
  }

  deletingProviderId.value = provider.id;
  error.value = "";
  actionMessage.value = "";

  try {
    await api.deleteProvider(provider.id);
    actionMessage.value = "提供方已删除。";
    if (draft.value.id === provider.id) {
      resetDraft();
    }
    await loadProviders();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "删除提供方失败。";
  } finally {
    deletingProviderId.value = null;
  }
}

onMounted(() => {
  void loadProviders();
});
</script>

<template>
  <section class="page-grid">
    <article class="list-panel">
      <div class="panel-header">
        <div class="panel-header-left">
          <h3 class="panel-title">已配置提供方</h3>
          <TinyTag
            v-if="loading"
            type="info"
          >
            加载中
          </TinyTag>
          <TinyTag
            v-else
            type="success"
          >
            {{ providers.length }} 项
          </TinyTag>
        </div>
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
        v-if="providers.length === 0 && !loading"
        class="empty-state"
      >
        <p>暂无已配置的提供方</p>
      </div>

      <div
        v-else
        class="provider-list"
      >
        <div
          v-for="provider in providers"
          :key="provider.id"
          class="provider-card"
        >
          <div class="provider-info">
            <div class="provider-header">
              <strong class="provider-name">{{ provider.display_name }}</strong>
              <TinyTag
                v-if="provider.is_default"
                type="success"
              >
                默认
              </TinyTag>
            </div>
            <span class="provider-url">{{ provider.base_url }}</span>
            <span class="provider-model">默认模型：{{ provider.default_model }}</span>
          </div>
          <div class="provider-actions">
            <TinyButton
              :data-testid="`test-provider-${provider.id}`"
              type="default"
              :disabled="testingProviderId === provider.id"
              @click="testProvider(provider)"
            >
              {{ testingProviderId === provider.id ? "测试中" : "测试连接" }}
            </TinyButton>
            <TinyButton
              :data-testid="`edit-provider-${provider.id}`"
              type="default"
              @click="editProvider(provider)"
            >
              编辑
            </TinyButton>
            <TinyButton
              :data-testid="`delete-provider-${provider.id}`"
              type="default"
              :disabled="deletingProviderId === provider.id"
              @click="deleteProvider(provider)"
            >
              {{ deletingProviderId === provider.id ? "删除中" : "删除" }}
            </TinyButton>
          </div>
        </div>
      </div>
    </article>

    <article class="form-panel">
      <div class="panel-header">
        <h3 class="panel-title">新增提供方</h3>
        <p class="panel-description">填写下方信息以添加新的模型提供方</p>
      </div>

      <ProviderForm
        v-model="draft"
        :submit-label="submitLabel"
        @cancel="resetDraft"
        @submit="saveProvider"
      />

      <div class="form-extra-actions">
        <TinyButton
          data-testid="test-provider-draft"
          type="default"
          :disabled="testingProviderId === 0"
          @click="testDraftProvider"
        >
          {{ testingProviderId === 0 ? "测试中" : "测试当前配置" }}
        </TinyButton>
      </div>

      <p
        v-if="error"
        class="error-message"
      >
        {{ error }}
      </p>
    </article>
  </section>
</template>

<style scoped>
.page-grid {
  display: grid;
  gap: var(--space-5);
  align-items: start;
  width: 100%;
}

.form-panel,
.list-panel {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  transition: border-color var(--transition-base);
}

.form-panel {
  width: 100%;
  padding: var(--space-5);
  display: grid;
  gap: var(--space-5);
}

.list-panel {
  padding: var(--space-5);
  display: grid;
  gap: var(--space-4);
}

.panel-header {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.panel-header-left {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.panel-title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
}

.panel-description {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.provider-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.provider-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--space-4);
  padding: var(--space-4);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  transition:
    border-color var(--transition-base),
    transform var(--transition-fast);
}

.provider-card:hover {
  border-color: var(--border-default);
}

.provider-card:active {
  transform: scale(0.99);
}

.provider-info {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  flex: 1 1 320px;
  min-width: 0;
}

.provider-actions,
.form-extra-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.provider-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.provider-name {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
}

.provider-url {
  font-size: var(--text-xs);
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.provider-model {
  font-size: var(--text-xs);
  color: var(--text-placeholder);
}

.empty-state {
  padding: var(--space-8) var(--space-4);
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

.success-message {
  margin: 0;
  padding: var(--space-3);
  background: var(--success-bg);
  border: 1px solid var(--success-border);
  border-radius: var(--radius-md);
  color: var(--success);
  font-size: var(--text-sm);
}

@media (max-width: 1024px) {
  .page-grid {
    grid-template-columns: 1fr;
  }
}
</style>
