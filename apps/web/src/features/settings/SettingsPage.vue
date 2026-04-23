<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";

import { getApiClient, type UpdateSettingsRequest } from "@/lib/api";
import { TinyButton, TinyForm, TinyFormItem, TinyInput, TinyNumeric, TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const form = reactive<UpdateSettingsRequest>({
  instance_name: "",
  default_provider_id: null,
});
const savedProviderName = ref("");
const saving = ref(false);
const saveSuccess = ref(false);

onMounted(async () => {
  const settings = await api.getSettings();
  form.instance_name = settings.instance_name;
  form.default_provider_id = settings.default_provider_id;
  savedProviderName.value = settings.default_provider_name ?? "未设置";
});

async function saveSettings() {
  saving.value = true;
  saveSuccess.value = false;

  try {
    const saved = await api.updateSettings({ ...form });
    savedProviderName.value = saved.default_provider_name ?? "未设置";
    saveSuccess.value = true;
    setTimeout(() => {
      saveSuccess.value = false;
    }, 3000);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <section class="page-section">
    <div class="settings-card">
      <div class="card-header">
        <h3 class="card-title">实例偏好</h3>
        <TinyTag
          :type="saveSuccess ? 'success' : 'info'"
        >
          {{ saveSuccess ? "已保存" : saving ? "保存中" : "就绪" }}
        </TinyTag>
      </div>

      <div class="settings-summary">
        <div class="summary-item">
          <span class="summary-label">当前实例</span>
          <span class="summary-value">{{ form.instance_name || '未命名实例' }}</span>
        </div>
        <div class="summary-item">
          <span class="summary-label">默认提供方</span>
          <span class="summary-value">{{ savedProviderName }}</span>
        </div>
      </div>

      <TinyForm class="settings-form">
        <TinyFormItem label="实例名称">
          <TinyInput
            v-model="form.instance_name"
            name="instance-name"
            placeholder="例如：Workspace Admin"
          />
        </TinyFormItem>

        <TinyFormItem label="默认提供方 ID">
          <TinyNumeric
            v-model="form.default_provider_id"
            name="default-provider-id"
          />
        </TinyFormItem>
      </TinyForm>

      <div class="card-actions">
        <TinyButton
          type="primary"
          :disabled="saving"
          @click="saveSettings"
        >
          {{ saving ? "保存中…" : "保存设置" }}
        </TinyButton>
      </div>
    </div>
  </section>
</template>

<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.settings-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
  padding: var(--space-6);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.card-title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
}

.settings-summary {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
  padding: var(--space-4);
  background: var(--surface-raised);
  border-radius: var(--radius-md);
}

.summary-item {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.summary-label {
  font-size: var(--text-xs);
  font-weight: 510;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.summary-value {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
}

.settings-form {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.card-actions {
  display: flex;
  justify-content: flex-end;
  padding-top: var(--space-4);
  border-top: 1px solid var(--border-subtle);
}

@media (max-width: 960px) {
  .settings-form {
    grid-template-columns: 1fr;
  }
}
</style>
