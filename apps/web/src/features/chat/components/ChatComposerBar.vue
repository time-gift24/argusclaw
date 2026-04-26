<script setup lang="ts">
import { computed, ref } from "vue";
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

// TrSender theme vars — transparent, borderless, clean
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
  // Submit button
  "--tr-sender-button-bg": "var(--accent)",
  "--tr-sender-button-bg-hover": "var(--accent-hover)",
  "--tr-sender-button-text-color": "#ffffff",
  "--tr-sender-button-border-radius": "10px",
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

// Dropdown open state for visual feedback
const templateDropdownOpen = ref(false);
const providerDropdownOpen = ref(false);
</script>

<template>
  <div class="composer-bar composer-bar--dock">
    <!-- 输入区 -->
    <div class="composer-bar__input-shell">
      <TrSender
        :model-value="modelValue"
        class="composer-bar__sender"
        mode="single"
        size="small"
        :style="senderThemeStyle"
        :clearable="true"
        :disabled="disabled"
        :loading="loading"
        :placeholder="placeholder"
        stop-text="停止"
        @update:model-value="emit('update:modelValue', $event)"
        @submit="handleSubmit"
        @cancel="handleCancel"
      />
    </div>

    <!-- 底部控制栏 -->
    <div class="composer-bar__footer-row">
      <!-- 智能体 -->
      <TinySelect
        class="composer-bar__select"
        :model-value="selectedTemplateValue"
        placeholder="智能体"
        size="small"
        :dropdown-class="templateDropdownOpen ? 'is-open' : ''"
        @update:model-value="handleTemplateChange"
        @dropdown-open-change="(v: boolean) => (templateDropdownOpen = v)"
      >
        <TinyOption
          v-for="opt in templateOptions"
          :key="opt.value"
          :label="opt.label"
          :value="opt.value"
        />
      </TinySelect>

      <!-- 提供方 -->
      <TinySelect
        class="composer-bar__select"
        :model-value="selectedProviderValue"
        placeholder="提供方"
        size="small"
        :dropdown-class="providerDropdownOpen ? 'is-open' : ''"
        @update:model-value="handleProviderChange"
        @dropdown-open-change="(v: boolean) => (providerDropdownOpen = v)"
      >
        <TinyOption
          v-for="opt in providerOptions"
          :key="opt.value"
          :label="opt.label"
          :value="opt.value"
        />
      </TinySelect>

      <!-- 模型名 -->
      <input
        class="composer-bar__model-input"
        type="text"
        :value="selectedModel"
        placeholder="模型"
        @input="handleModelChange(($event.target as HTMLInputElement).value)"
      />

      <div class="composer-bar__actions">
        <button class="composer-bar__icon-btn" title="新对话" @click="emit('newChat')">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2">
            <path d="M12 5v14M5 12h14" />
          </svg>
          <span>新对话</span>
        </button>
        <button class="composer-bar__icon-btn" title="历史会话" @click="emit('openHistory')">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2">
            <circle cx="12" cy="12" r="9" />
            <path d="M12 7v5l3 3" />
          </svg>
          <span>历史</span>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* ── 外壳：dock 悬浮感 ── */
.composer-bar {
  padding: 14px 16px 12px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.composer-bar--dock {
  border-radius: 20px;
  background: rgba(255, 255, 255, 0.88);
  backdrop-filter: blur(20px) saturate(180%);
  -webkit-backdrop-filter: blur(20px) saturate(180%);
  box-shadow:
    0 8px 32px rgba(15, 23, 42, 0.08),
    0 2px 8px rgba(15, 23, 42, 0.05),
    inset 0 1px 0 rgba(255, 255, 255, 0.8);
  border: 1px solid rgba(255, 255, 255, 0.6);
}

/* ── 输入区 ── */
.composer-bar__input-shell {
  display: flex;
  align-items: stretch;
  min-width: 0;
}

.composer-bar__sender {
  flex: 1;
  border: 0 !important;
  border-radius: 0 !important;
  background: transparent !important;
  box-shadow: none !important;
}

.composer-bar__sender :deep(.tr-sender) {
  min-height: 0;
  background: transparent;
  border: 0;
  box-shadow: none;
  padding: 0;
}

.composer-bar__sender :deep(.tr-sender-editor-scroll) {
  overflow-y: auto;
}

.composer-bar__sender :deep(.tr-sender-editor-content .ProseMirror),
.composer-bar__sender :deep(.tr-sender-editor-content .ProseMirror p) {
  font-size: var(--text-base);
  line-height: 1.6;
  color: var(--text-primary);
}

.composer-bar__sender :deep(.tr-sender-editor-content .ProseMirror p.is-editor-empty:first-child::before) {
  color: var(--text-placeholder);
  content: attr(data-placeholder);
  float: left;
  height: 0;
  pointer-events: none;
}

/* 发送按钮 */
.composer-bar__sender :deep(.tr-sender-button) {
  background: var(--accent) !important;
  border: none !important;
  border-radius: 10px !important;
  width: 38px !important;
  height: 38px !important;
  margin: 2px 4px 2px 0 !important;
  transition: background 0.12s ease, transform 0.1s ease, box-shadow 0.12s ease !important;
  display: flex !important;
  align-items: center !important;
  justify-content: center !important;
  box-shadow: 0 2px 8px rgba(94, 106, 210, 0.25) !important;
}

.composer-bar__sender :deep(.tr-sender-button:hover) {
  background: var(--accent-hover) !important;
  transform: scale(1.04);
  box-shadow: 0 4px 12px rgba(94, 106, 210, 0.35) !important;
}

.composer-bar__sender :deep(.tr-sender-button:active) {
  transform: scale(0.96);
}

/* ── 底部控制栏 ── */
.composer-bar__footer-row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding-bottom: 2px;
}

/* ── TinySelect：文字链接风格 ── */
.composer-bar__select {
  flex: 1;
  min-width: 0;
  max-width: 180px;
}

/* 触发器：去掉所有边框和小箭头 */
.composer-bar__select :deep(.tiny-select) {
  border: none !important;
  background: transparent !important;
  box-shadow: none !important;
  border-radius: 6px !important;
  height: 30px !important;
  min-width: unset !important;
  width: 100% !important;
  transition: background 0.12s ease !important;
}

.composer-bar__select :deep(.tiny-select:hover),
.composer-bar__select :deep(.tiny-select.is-open) {
  background: rgba(94, 106, 210, 0.07) !important;
}

.composer-bar__select :deep(.tiny-select__input) {
  border: none !important;
  background: transparent !important;
  box-shadow: none !important;
  color: var(--text-secondary) !important;
  font-size: 12px !important;
  padding: 0 6px !important;
  height: 30px !important;
  line-height: 30px !important;
  min-width: unset !important;
  width: 100% !important;
}

/* 去掉下拉箭头 */
.composer-bar__select :deep(.tiny-select__suffix) {
  display: none !important;
}

/* 下拉面板 */
.composer-bar__select :deep(.tiny-select__dropdown) {
  border: 1px solid var(--border-default) !important;
  border-radius: 12px !important;
  box-shadow:
    0 12px 40px rgba(15, 23, 42, 0.12),
    0 4px 12px rgba(15, 23, 42, 0.06) !important;
  padding: 6px !important;
  min-width: 160px !important;
  background: var(--surface-base) !important;
  margin-top: 4px !important;
}

/* 下拉项 */
.composer-bar__select :deep(.tiny-select__option) {
  border-radius: 8px !important;
  padding: 8px 12px !important;
  font-size: 13px !important;
  color: var(--text-primary) !important;
  border: none !important;
  margin: 2px 0 !important;
  transition: background 0.1s ease, color 0.1s ease !important;
}

.composer-bar__select :deep(.tiny-select__option:hover),
.composer-bar__select :deep(.tiny-select__option.hover) {
  background: rgba(94, 106, 210, 0.1) !important;
  color: var(--accent) !important;
}

.composer-bar__select :deep(.tiny-select__option.selected) {
  background: rgba(94, 106, 210, 0.1) !important;
  color: var(--accent) !important;
  font-weight: 500 !important;
}

/* ── 模型输入框 ── */
.composer-bar__model-input {
  flex: 1;
  min-width: 64px;
  max-width: 140px;
  height: 30px;
  padding: 0 8px;
  background: transparent;
  border: none !important;
  border-radius: 6px !important;
  box-shadow: none !important;
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 30px;
  outline: none;
  transition: color 0.12s ease, background 0.12s ease;
}

.composer-bar__model-input:hover {
  color: var(--text-primary);
  background: rgba(94, 106, 210, 0.05);
}

.composer-bar__model-input:focus {
  color: var(--accent);
  background: rgba(94, 106, 210, 0.07);
}

.composer-bar__model-input::placeholder {
  color: var(--text-placeholder);
}

/* ── 动作按钮 ── */
.composer-bar__actions {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-left: 4px;
}

.composer-bar__icon-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 30px;
  padding: 0 10px;
  background: transparent;
  border: none;
  border-radius: 6px;
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 450;
  cursor: pointer;
  transition: background 0.12s ease, color 0.12s ease;
  white-space: nowrap;
}

.composer-bar__icon-btn:hover {
  background: rgba(94, 106, 210, 0.08);
  color: var(--accent);
}

.composer-bar__icon-btn:active {
  transform: scale(0.96);
}

.composer-bar__icon-btn svg {
  flex-shrink: 0;
  opacity: 0.7;
}

.composer-bar__icon-btn:hover svg {
  opacity: 1;
}

/* ── 响应式 ── */
@media (max-width: 960px) {
  .composer-bar {
    padding: 12px 14px 10px;
  }

  .composer-bar__footer-row {
    flex-wrap: wrap;
    gap: 4px;
  }

  .composer-bar__model-input {
    max-width: none;
  }
}
</style>
