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

const templateOptions = computed(() =>
  props.templates.map((t) => ({ label: t.display_name, value: t.id })),
);

const providerOptions = computed(() =>
  props.providers.map((p) => ({ label: p.display_name, value: p.id })),
);

function handleSubmit(text: string) {
  emit("submit", text);
}

function handleCancel() {
  emit("cancel");
}

function handleTemplateChange(value: number | null) {
  emit("update:selectedTemplateId", value);
}

function handleProviderChange(value: number | null) {
  emit("update:selectedProviderId", value);
  const provider = props.providers.find((p) => p.id === value);
  if (provider) {
    emit("update:selectedModel", provider.default_model);
  }
}

function handleModelChange(value: string) {
  emit("update:selectedModel", value);
}
</script>

<template>
  <div class="composer-bar shell-card">
    <!-- Header controls: template/provider + action buttons -->
    <div class="composer-bar__header">
      <div class="composer-bar__controls">
        <!-- Template selector -->
        <div class="composer-bar__control-group">
          <label class="composer-bar__control-label">智能体</label>
          <TinySelect
            :model-value="selectedTemplateId"
            placeholder="选择模板"
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
        </div>

        <!-- Provider selector -->
        <div class="composer-bar__control-group">
          <label class="composer-bar__control-label">提供方</label>
          <TinySelect
            :model-value="selectedProviderId"
            placeholder="选择提供方"
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
        </div>

        <!-- Model input -->
        <div class="composer-bar__control-group composer-bar__control-group--model">
          <label class="composer-bar__control-label">模型</label>
          <input
            class="composer-bar__model-input"
            type="text"
            :value="selectedModel"
            placeholder="模型名称"
            @input="handleModelChange(($event.target as HTMLInputElement).value)"
          />
        </div>
      </div>

      <!-- Action buttons -->
      <div class="composer-bar__actions">
        <TinyButton size="small" title="新对话" @click="emit('newChat')">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12 5v14M5 12h14" />
          </svg>
          新对话
        </TinyButton>
        <TinyButton size="small" title="历史会话" @click="emit('openHistory')">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          历史
        </TinyButton>
      </div>
    </div>

    <!-- Sender -->
    <TrSender
      :model-value="modelValue"
      class="composer-bar__sender"
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
</template>

<style scoped>
.composer-bar {
  padding: var(--space-4, 16px);
  display: flex;
  flex-direction: column;
  gap: var(--space-3, 12px);
}

.composer-bar__header {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: var(--space-4, 16px);
  flex-wrap: wrap;
}

.composer-bar__controls {
  display: flex;
  align-items: flex-end;
  gap: var(--space-3, 12px);
  flex: 1;
  flex-wrap: wrap;
  min-width: 0;
}

.composer-bar__control-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 120px;
}

.composer-bar__control-group--model {
  min-width: 100px;
  flex: 1;
  max-width: 200px;
}

.composer-bar__control-label {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted, #8b919d);
  text-transform: uppercase;
  letter-spacing: 0.06em;
}

.composer-bar__model-input {
  height: 28px;
  padding: 0 var(--space-2, 8px);
  background: var(--input-bg, #fff);
  border: 1px solid var(--border-default, #e2e5eb);
  border-radius: var(--radius-sm, 4px);
  color: var(--text-primary, #1a1d23);
  font-size: 13px;
  outline: none;
  transition: border-color 0.15s;
  width: 100%;
}

.composer-bar__model-input:focus {
  border-color: var(--accent, #5e6ad2);
}

.composer-bar__model-input::placeholder {
  color: var(--text-placeholder, #a8aeb8);
}

.composer-bar__actions {
  display: flex;
  gap: var(--space-2, 8px);
  flex-shrink: 0;
}

.composer-bar__sender {
  border: 1px solid var(--border-default, #e2e5eb);
  border-radius: var(--radius-lg, 8px);
}
</style>
