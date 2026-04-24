<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";

import ProviderForm from "./ProviderForm.vue";
import { getApiClient, type LlmProviderRecord } from "@/lib/api";
import { TinyButton } from "@/lib/opentiny";

const api = getApiClient();
const route = useRoute();
const router = useRouter();

const isEdit = ref(false);

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

const saving = ref(false);
const loading = ref(true);
const error = ref("");
const actionMessage = ref("");
const testingProviderId = ref<number | null>(null);
const draft = ref<LlmProviderRecord>(createDraft());

const submitLabel = computed(() => {
  if (saving.value) {
    return isEdit.value ? "更新中…" : "保存中…";
  }
  return isEdit.value ? "更新提供方" : "创建提供方";
});

async function loadProvider() {
  isEdit.value = !!route.params.providerId;
  if (!isEdit.value) {
    loading.value = false;
    return;
  }

  loading.value = true;
  error.value = "";

  try {
    const providers = await api.listProviders();
    const providerId = parseInt(route.params.providerId as string, 10);
    const found = providers.find(p => p.id === providerId);
    if (found) {
      draft.value = { ...found, api_key: "" };
    } else {
      error.value = "未找到该提供方。";
    }
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载提供方失败。";
  } finally {
    loading.value = false;
  }
}

async function saveProvider() {
  saving.value = true;
  error.value = "";

  try {
    await api.saveProvider(draft.value);
    router.push("/providers");
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "保存提供方失败。";
  } finally {
    saving.value = false;
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

function goBack() {
  router.push("/providers");
}

onMounted(() => {
  void loadProvider();
});

watch(
  () => route.params.providerId,
  () => {
    void loadProvider();
  }
);
</script>

<template>
  <div class="edit-page">
    <article class="form-panel">
      <div class="panel-header">
        <h3 class="panel-title">{{ isEdit ? '编辑提供方' : '新增提供方' }}</h3>
        <p class="panel-description">
          {{ isEdit ? '修改现有模型提供方的接入凭据与配置' : '填写下方信息以添加新的模型提供方' }}
        </p>
      </div>

      <p v-if="loading" class="loading-message">加载中...</p>

      <template v-else>
        <ProviderForm
          v-model="draft"
          :submit-label="submitLabel"
          @cancel="goBack"
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

        <p v-if="error" class="error-message">{{ error }}</p>
        <p v-if="actionMessage" class="success-message">{{ actionMessage }}</p>
      </template>
    </article>
  </div>
</template>

<style scoped>
.edit-page {
  max-width: 800px;
}

.form-panel {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  padding: var(--space-5);
  display: grid;
  gap: var(--space-5);
}

.panel-header {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
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

.form-extra-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.loading-message {
  text-align: center;
  color: var(--text-muted);
  padding: var(--space-5);
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
</style>
