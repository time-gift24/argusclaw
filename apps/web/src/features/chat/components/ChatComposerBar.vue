<script setup lang="ts">
import { computed } from "vue";
import { TrSender } from "@opentiny/tiny-robot";
import { TinyButton, TinyOption, TinySelect } from "@/lib/opentiny";
import type { AgentRecord, LlmProviderRecord } from "@/lib/api";

interface Props {
  modelValue: string;
  templates: AgentRecord[];
  providers: LlmProviderRecord[];
  selectedTemplateId: number | null;
  selectedProviderId: number | null;
  selectedModel: string;
  disabled: boolean;
  loading: boolean;
  placeholder: string;
  hasActiveThread: boolean;
  activeProvider: LlmProviderRecord | null;
  selectedTemplate: AgentRecord | null;
}

interface Emits {
  (e: "update:modelValue", value: string): void;
  (e: "submit", value: string): void;
  (e: "cancel"): void;
  (e: "newChat"): void;
  (e: "openHistory"): void;
  (e: "update:selectedTemplateId", value: number | null): void;
  (e: "update:selectedProviderId", value: number | null): void;
  (e: "update:selectedModel", value: string): void;
}

const props = defineProps<Props>();
const emit = defineEmits<Emits>();

type SelectValue = string | number | null | undefined;

const templateOptions = computed(() =>
  props.templates.map((t) => ({ label: t.display_name, value: String(t.id) })),
);

const providerOptions = computed(() =>
  props.providers.map((p) => ({ label: p.display_name, value: String(p.id) })),
);

const selectedTemplateValue = computed(() => toSelectValue(props.selectedTemplateId));
const selectedProviderValue = computed(() => toSelectValue(props.selectedProviderId));
const senderThemeStyle = {
  "--tr-sender-bg-color": "transparent",
  "--tr-sender-bg-color-disabled": "transparent",
  "--tr-sender-text-color": "var(--text-primary)",
  "--tr-sender-text-color-disabled": "var(--text-muted)",
  "--tr-sender-placeholder-color": "var(--text-placeholder)",
  "--tr-sender-placeholder-color-disabled": "var(--text-placeholder)",
  "--tr-sender-border-radius": "0px",
  "--tr-sender-border-radius-small": "0px",
  "--tr-sender-font-size": "var(--text-base)",
  "--tr-sender-font-size-small": "var(--text-base)",
  "--tr-sender-line-height": "22px",
  "--tr-sender-line-height-small": "22px",
  "--tr-sender-padding-small": "0px",
  "--tr-sender-header-padding-small": "0px",
  "--tr-sender-multi-main-padding-small": "0px",
  "--tr-sender-footer-padding-small": "0px",
  "--tr-sender-button-size-small": "28px",
  "--tr-sender-button-size-submit-small": "34px",
  "--tr-sender-actions-padding-right-small": "6px",
  "--tr-sender-box-shadow": "none",
} as const;

function toSelectValue(value: number | null) {
  return value === null ? "" : String(value);
}

function parseSelectId(value: SelectValue) {
  if (value === null || value === undefined || value === "") return null;
  const next = Number(value);
  return Number.isFinite(next) ? next : null;
}

function handleSubmit(text: string) {
  emit("submit", text);
}

function handleCancel() {
  emit("cancel");
}

function handleTemplateChange(value: SelectValue) {
  emit("update:selectedTemplateId", parseSelectId(value));
}

function handleProviderChange(value: SelectValue) {
  const providerId = parseSelectId(value);
  emit("update:selectedProviderId", providerId);
  const provider = props.providers.find((p) => p.id === providerId);
  if (provider) {
    emit("update:selectedModel", provider.default_model);
  }
}

function handleModelChange(value: string) {
  emit("update:selectedModel", value);
}
</script>

<template>
  <div class="composer-bar composer-bar--dock">
    <div class="composer-bar__input-shell">
      <TrSender
        :model-value="modelValue"
        class="composer-bar__sender composer-bar__sender--borderless"
        mode="single"
        size="small"
        :style="senderThemeStyle"
        :clearable="true"
        :disabled="disabled"
        :loading="loading"
        :placeholder="placeholder"
        stop-text="取消运行"
        @update:model-value="emit('update:modelValue', $event)"
        @submit="handleSubmit"
        @cancel="handleCancel"
      />
    </div>

    <div class="composer-bar__footer-row composer-bar__footer-row--compact">
      <TinySelect
        class="composer-bar__plain-control composer-bar__plain-select"
        :model-value="selectedTemplateValue"
        placeholder="智能体"
        size="small"
        @update:model-value="handleTemplateChange"
      >
        <TinyOption
          v-for="opt in templateOptions"
          :key="opt.value"
          :label="opt.label"
          :value="opt.value"
        />
      </TinySelect>

      <TinySelect
        class="composer-bar__plain-control composer-bar__plain-select"
        :model-value="selectedProviderValue"
        placeholder="提供方"
        size="small"
        @update:model-value="handleProviderChange"
      >
        <TinyOption
          v-for="opt in providerOptions"
          :key="opt.value"
          :label="opt.label"
          :value="opt.value"
        />
      </TinySelect>

      <input
        class="composer-bar__plain-control composer-bar__model-input"
        type="text"
        :value="selectedModel"
        placeholder="模型"
        @input="handleModelChange(($event.target as HTMLInputElement).value)"
      />

      <TinyButton class="composer-bar__plain-control composer-bar__action-btn" size="small" title="新对话" @click="emit('newChat')">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 5v14M5 12h14" />
        </svg>
        新对话
      </TinyButton>
      <TinyButton class="composer-bar__plain-control composer-bar__action-btn" size="small" title="历史会话" @click="emit('openHistory')">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        历史
      </TinyButton>
    </div>
  </div>
</template>

<style scoped>
.composer-bar {
  padding: 14px 16px 12px;
  display: flex;
  flex-direction: column;
  gap: var(--space-3, 12px);
}

.composer-bar--dock {
  border-radius: 30px;
  background: rgba(255, 255, 255, 0.9);
  backdrop-filter: blur(22px);
  box-shadow:
    0 20px 44px rgba(15, 23, 42, 0.1),
    inset 0 1px 0 rgba(255, 255, 255, 0.74);
}

.composer-bar__input-shell {
  display: flex;
  align-items: stretch;
  min-width: 0;
  padding: 4px 2px 0;
}

.composer-bar__footer-row {
  display: grid;
  align-items: center;
  grid-template-columns: minmax(76px, 0.9fr) minmax(76px, 0.9fr) minmax(64px, 1fr) auto auto;
  gap: 8px;
  min-width: 0;
  padding-bottom: 2px;
}

.composer-bar__plain-control {
  height: 32px;
  padding: 0;
  background: transparent;
  border: 0;
  border-radius: 0;
  box-shadow: none;
  color: var(--text-secondary, #4b5563);
  font-size: var(--text-sm);
  line-height: 32px;
}

.composer-bar__model-input {
  min-width: 64px;
  width: 100%;
  outline: none;
  transition:
    color 0.15s,
    opacity 0.15s;
}

.composer-bar__model-input:focus {
  color: var(--text-primary, #1a1d23);
}

.composer-bar__model-input::placeholder {
  color: var(--text-placeholder, #a8aeb8);
}

.composer-bar__action-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.composer-bar__sender {
  flex: 1;
  border: 0;
  border-radius: 0;
  background: transparent;
  box-shadow: none;
}

:deep(.composer-bar__sender .tr-sender) {
  min-height: 0;
  background: transparent;
  border: 0;
  box-shadow: none;
}

:deep(.composer-bar__sender .tr-sender-editor-scroll) {
  overflow-y: auto;
}

:deep(.composer-bar__sender .tr-sender-editor-content .ProseMirror),
:deep(.composer-bar__sender .tr-sender-editor-content .ProseMirror p) {
  font-size: var(--text-base);
  line-height: 1.6;
}

:deep(.composer-bar__plain-select .tiny-select__input) {
  min-height: 32px;
  width: 100% !important;
  background: transparent !important;
  border: 0 !important;
  box-shadow: none !important;
  padding: 0 !important;
  color: var(--text-secondary, #4b5563) !important;
}

:deep(.composer-bar__plain-select) {
  width: 100% !important;
}

:deep(.composer-bar__plain-select .tiny-select__suffix),
:deep(.composer-bar__plain-select .tiny-input__suffix) {
  color: var(--text-muted);
}

:deep(.composer-bar__action-btn.tiny-button) {
  padding: 0 !important;
  height: 32px !important;
  background: transparent !important;
  border: 0 !important;
  box-shadow: none !important;
  color: var(--text-secondary, #4b5563) !important;
}

@media (max-width: 960px) {
  .composer-bar {
    padding: 12px 14px 10px;
  }

  .composer-bar__footer-row {
    grid-template-columns: minmax(68px, 0.9fr) minmax(68px, 0.9fr) minmax(52px, 1fr) auto auto;
    gap: 6px;
  }

  .composer-bar__model-input {
    max-width: none;
  }
}
</style>
