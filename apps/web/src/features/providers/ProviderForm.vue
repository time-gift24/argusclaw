<script setup lang="ts">
import { computed, ref, watch } from "vue";

import type { LlmProviderRecord } from "@/lib/api";
import {
  TinyButton,
  TinyForm,
  TinyFormItem,
  TinyInput,
  TinyOption,
  TinySelect,
  TinySwitch,
} from "@/lib/opentiny";

const props = defineProps<{
  modelValue: LlmProviderRecord;
  submitLabel?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: LlmProviderRecord];
  submit: [];
  cancel: [];
}>();

function cloneProvider(value: LlmProviderRecord): LlmProviderRecord {
  return {
    ...value,
    extra_headers: { ...value.extra_headers },
    meta_data: { ...value.meta_data },
    model_config: { ...value.model_config },
    models: [...value.models],
  };
}

const draft = ref<LlmProviderRecord>(cloneProvider(props.modelValue));
const modelsText = ref(props.modelValue.models.join(", "));
const isEditing = computed(() => draft.value.id > 0);
const usesAccountTokenSource = computed(
  () => draft.value.meta_data.account_token_source === "true",
);

function parseModels(input: string) {
  return input
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function emitDraft(next: LlmProviderRecord) {
  emit("update:modelValue", {
    ...cloneProvider(next),
    models: parseModels(modelsText.value),
  });
}

function updateField<K extends keyof LlmProviderRecord>(key: K, value: LlmProviderRecord[K]) {
  const next = {
    ...draft.value,
    [key]: value,
  };

  draft.value = next;
  emitDraft(next);
}

function updateModels(value: string | number) {
  modelsText.value = String(value);
  emitDraft(draft.value);
}

function updateAccountTokenSource(value: boolean) {
  const metaData = { ...draft.value.meta_data };
  if (value) {
    metaData.account_token_source = "true";
  } else {
    delete metaData.account_token_source;
  }

  const next = {
    ...draft.value,
    meta_data: metaData,
  };
  draft.value = next;
  emitDraft(next);
}

watch(
  () => props.modelValue,
  (value) => {
    draft.value = cloneProvider(value);
    modelsText.value = value.models.join(", ");
  },
  { immediate: true },
);
</script>

<template>
  <form
    class="provider-form"
    @submit.prevent="emit('submit')"
  >
    <TinyForm
      label-position="top"
      class="provider-form__grid"
    >
      <TinyFormItem label="提供方名称">
        <TinyInput
          :model-value="draft.display_name"
          name="display-name"
          placeholder="例如：OpenAI"
          @update:model-value="updateField('display_name', $event)"
        />
      </TinyFormItem>

      <TinyFormItem label="提供方类型">
        <TinySelect
          :model-value="draft.kind"
          name="provider-kind"
          @update:model-value="updateField('kind', $event)"
        >
          <TinyOption
            label="OpenAI 兼容"
            value="openai-compatible"
          />
        </TinySelect>
      </TinyFormItem>

      <TinyFormItem label="服务地址">
        <TinyInput
          :model-value="draft.base_url"
          name="base-url"
          placeholder="https://api.openai.com/v1"
          @update:model-value="updateField('base_url', $event)"
        />
      </TinyFormItem>

      <TinyFormItem label="API Key">
        <TinyInput
          :model-value="draft.api_key"
          name="api-key"
          placeholder="sk-..."
          type="password"
          @update:model-value="updateField('api_key', $event)"
        />
      </TinyFormItem>

      <TinyFormItem
        label="账号 Token 鉴权"
        class="full-width"
      >
        <div class="provider-form__switch">
          <TinySwitch
            :model-value="usesAccountTokenSource"
            name="account-token-source"
            @update:model-value="updateAccountTokenSource"
          />
          <span class="switch-hint">
            开启后该提供方会在服务端运行时使用已配置账号换取 token，API Key 可留空。
          </span>
        </div>
      </TinyFormItem>

      <TinyFormItem label="模型列表">
        <TinyInput
          :model-value="modelsText"
          name="models"
          placeholder="gpt-4.1, gpt-4.1-mini"
          @update:model-value="updateModels"
        />
      </TinyFormItem>

      <TinyFormItem label="默认模型">
        <TinyInput
          :model-value="draft.default_model"
          name="default-model"
          placeholder="gpt-4.1"
          @update:model-value="updateField('default_model', $event)"
        />
      </TinyFormItem>

      <TinyFormItem
        label="设为默认"
        class="full-width"
      >
        <div class="provider-form__switch">
          <TinySwitch
            :model-value="draft.is_default"
            name="is-default"
            @update:model-value="updateField('is_default', $event)"
          />
          <span class="switch-hint">
            {{ isEditing ? "更新该提供方的默认状态" : "将该提供方设为实例默认项" }}
          </span>
        </div>
      </TinyFormItem>
    </TinyForm>

    <div class="provider-form__actions">
      <TinyButton
        native-type="submit"
        type="primary"
      >
        {{ submitLabel ?? "保存提供方" }}
      </TinyButton>
      <TinyButton
        type="default"
        @click="emit('cancel')"
      >
        {{ isEditing ? "取消编辑" : "重置" }}
      </TinyButton>
    </div>
  </form>
</template>

<style scoped>
.provider-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}

.provider-form__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.provider-form__grid :deep(.tiny-form-item) {
  margin-bottom: 0;
}

.provider-form__switch {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.switch-hint {
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.provider-form__actions {
  display: flex;
  gap: var(--space-3);
  flex-wrap: wrap;
}

.full-width {
  grid-column: 1 / -1;
}

@media (max-width: 960px) {
  .provider-form__grid {
    grid-template-columns: 1fr;
  }
}
</style>
