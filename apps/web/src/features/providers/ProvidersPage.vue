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
const draft = ref<LlmProviderRecord>(createDraft());

const submitLabel = computed(() => {
  if (saving.value) {
    return draft.value.id > 0 ? "更新中…" : "保存中…";
  }

  return draft.value.id > 0 ? "更新提供方" : "创建提供方";
});

async function loadProviders() {
  loading.value = true;

  try {
    providers.value = await api.listProviders();
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

  try {
    await api.saveProvider(draft.value);
    resetDraft();
    await loadProviders();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "保存提供方失败。";
  } finally {
    saving.value = false;
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
          <TinyButton
            :data-testid="`edit-provider-${provider.id}`"
            type="default"
            @click="editProvider(provider)"
          >
            编辑
          </TinyButton>
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

@media (max-width: 1024px) {
  .page-grid {
    grid-template-columns: 1fr;
  }
}
</style>
