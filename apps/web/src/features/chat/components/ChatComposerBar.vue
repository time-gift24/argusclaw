<script setup lang="ts">
import { computed, ref } from "vue";
import { TrSender } from "@opentiny/tiny-robot";
import { IconAi, IconThink } from "@opentiny/tiny-robot-svgs/dist/tiny-robot-svgs.js";
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

const modelOptions = computed(() =>
  (props.activeProvider?.models ?? []).map((model) => ({ label: model, value: model })),
);

const selectedTemplateLabel = computed(() => props.selectedTemplate?.display_name ?? "选择 Agent");
const selectedProviderLabel = computed(() => props.activeProvider?.display_name ?? "选择提供方");
const selectedModelLabel = computed(() => props.selectedModel || "选择模型");

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
  activePicker.value = null;
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
  activePicker.value = null;
}

const activePicker = ref<"agent" | "llm" | null>(null);

function togglePicker(picker: "agent" | "llm") {
  activePicker.value = activePicker.value === picker ? null : picker;
}
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
      <div class="composer-bar__picker">
        <button
          type="button"
          class="composer-bar__picker-trigger composer-bar__picker-trigger--single"
          data-testid="agent-picker-trigger"
          :aria-expanded="activePicker === 'agent'"
          @click="togglePicker('agent')"
        >
          <span class="composer-bar__picker-kicker composer-bar__picker-kicker--agent">
            <IconAi class="composer-bar__picker-icon composer-bar__picker-icon--agent" aria-hidden="true" />
            Agent
          </span>
          <strong>{{ selectedTemplateLabel }}</strong>
        </button>
        <div
          v-if="activePicker === 'agent'"
          class="composer-bar__popover composer-bar__popover--agent"
          data-testid="agent-picker-popover"
        >
          <button
            v-for="template in templates"
            :key="template.id"
            type="button"
            class="composer-bar__option"
            :class="{ 'is-selected': template.id === selectedTemplateId }"
            :data-testid="`agent-option-${template.id}`"
            @click="handleTemplateChange(template.id)"
          >
            <span>{{ template.display_name }}</span>
            <small v-if="template.description">{{ template.description }}</small>
          </button>
        </div>
      </div>

      <div class="composer-bar__picker composer-bar__picker--llm">
        <button
          type="button"
          class="composer-bar__picker-trigger"
          data-testid="llm-picker-trigger"
          :aria-expanded="activePicker === 'llm'"
          @click="togglePicker('llm')"
        >
          <span class="composer-bar__picker-kicker">
            <IconThink class="composer-bar__picker-icon" aria-hidden="true" />
            LLM
          </span>
          <strong>{{ selectedProviderLabel }}</strong>
          <small>{{ selectedModelLabel }}</small>
        </button>
        <div
          v-if="activePicker === 'llm'"
          class="composer-bar__popover composer-bar__popover--llm"
          data-testid="llm-picker-popover"
        >
          <div class="composer-bar__provider-column">
            <button
              v-for="provider in providers"
              :key="provider.id"
              type="button"
              class="composer-bar__option composer-bar__option--provider"
              :class="{ 'is-selected': provider.id === selectedProviderId }"
              :data-testid="`provider-option-${provider.id}`"
              @click="handleProviderChange(provider.id)"
            >
              <span>{{ provider.display_name }}</span>
              <small>{{ provider.default_model }}</small>
            </button>
          </div>
          <div class="composer-bar__model-column">
            <button
              v-for="opt in modelOptions"
              :key="opt.value"
              type="button"
              class="composer-bar__option composer-bar__option--model"
              :class="{ 'is-selected': opt.value === selectedModel }"
              :data-testid="`model-option-${opt.value}`"
              @click="handleModelChange(opt.value)"
            >
              {{ opt.label }}
            </button>
            <span v-if="modelOptions.length === 0" class="composer-bar__empty-option">暂无模型</span>
          </div>
        </div>
      </div>

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
  border: 1px solid color-mix(in srgb, var(--border-default) 76%, rgba(255, 255, 255, 0.6));
  box-shadow:
    0 8px 32px rgba(15, 23, 42, 0.08),
    0 2px 8px rgba(15, 23, 42, 0.05),
    inset 0 1px 0 rgba(255, 255, 255, 0.8);
  transition:
    border-color 0.16s ease,
    box-shadow 0.16s ease,
    background-color 0.16s ease;
}

.composer-bar--dock:focus-within {
  border-color: color-mix(in srgb, var(--accent) 62%, var(--border-default));
  background: var(--surface-base);
  box-shadow:
    0 10px 34px rgba(15, 23, 42, 0.1),
    0 0 0 3px color-mix(in srgb, var(--accent) 12%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.75);
}

/* ── 输入区 ── */
.composer-bar__input-shell {
  display: flex;
  align-items: stretch;
  min-width: 0;
  padding: 8px 10px;
  border-radius: 14px;
  background: color-mix(in srgb, var(--surface-base) 62%, transparent);
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
  position: relative;
  display: flex;
  align-items: center;
  gap: 6px;
  padding-bottom: 2px;
}

.composer-bar__picker {
  position: relative;
  min-width: 0;
}

.composer-bar__picker--llm {
  flex: 1;
}

.composer-bar__picker-trigger {
  display: inline-grid;
  grid-template-columns: auto minmax(0, 1fr);
  align-items: center;
  column-gap: 8px;
  row-gap: 1px;
  min-width: 134px;
  max-width: 250px;
  height: 34px;
  padding: 0 12px;
  border: 1px solid color-mix(in srgb, var(--border-default) 76%, transparent);
  border-radius: 10px;
  background: color-mix(in srgb, var(--surface-base) 72%, transparent);
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
  transition:
    background 0.14s ease,
    border-color 0.14s ease,
    box-shadow 0.14s ease,
    transform 0.1s ease;
}

.composer-bar__picker-trigger:hover,
.composer-bar__picker-trigger[aria-expanded="true"] {
  border-color: color-mix(in srgb, var(--accent) 40%, var(--border-default));
  background: color-mix(in srgb, var(--accent) 8%, var(--surface-base));
  box-shadow: 0 6px 18px rgba(15, 23, 42, 0.08);
}

.composer-bar__picker-trigger:active {
  transform: scale(0.98);
}

.composer-bar__picker-trigger strong,
.composer-bar__picker-trigger small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.composer-bar__picker-trigger strong {
  font-size: 12px;
  font-weight: 650;
  line-height: 1.2;
}

.composer-bar__picker-trigger--single strong {
  grid-row: 1 / span 2;
  align-self: center;
}

.composer-bar__picker-trigger small {
  grid-column: 2;
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.1;
}

.composer-bar__picker-kicker {
  grid-row: 1 / span 2;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  height: 20px;
  padding: 0 7px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0;
  text-transform: uppercase;
}

.composer-bar__picker-kicker--agent {
  background: color-mix(in srgb, var(--accent) 9%, #ffffff);
  color: var(--text-primary);
}

.composer-bar__picker-icon {
  width: 12px;
  height: 12px;
  display: block;
  flex: 0 0 auto;
}

.composer-bar__picker-icon--agent {
  width: 14px;
  height: 14px;
}

.composer-bar__picker-icon :deep(svg),
.composer-bar__picker-kicker :deep(svg) {
  width: 12px;
  height: 12px;
  display: block;
}

.composer-bar__picker-kicker--agent :deep(svg) {
  width: 14px;
  height: 14px;
}

.composer-bar__popover {
  position: absolute;
  left: 0;
  bottom: calc(100% + 8px);
  z-index: 50;
  min-width: 260px;
  padding: 8px;
  border: 1px solid color-mix(in srgb, var(--border-default) 78%, transparent);
  border-radius: 14px;
  background: rgba(255, 255, 255, 0.96);
  box-shadow:
    0 20px 48px rgba(15, 23, 42, 0.16),
    0 4px 14px rgba(15, 23, 42, 0.08);
  backdrop-filter: blur(18px) saturate(160%);
}

.composer-bar__popover--llm {
  display: grid;
  grid-template-columns: minmax(150px, 0.95fr) minmax(160px, 1fr);
  gap: 8px;
  min-width: 390px;
}

.composer-bar__provider-column,
.composer-bar__model-column {
  display: grid;
  align-content: start;
  gap: 4px;
}

.composer-bar__model-column {
  border-left: 1px solid color-mix(in srgb, var(--border-default) 72%, transparent);
  padding-left: 8px;
}

.composer-bar__option {
  display: grid;
  gap: 2px;
  width: 100%;
  padding: 9px 10px;
  border: 0;
  border-radius: 9px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
  transition:
    background 0.12s ease,
    color 0.12s ease;
}

.composer-bar__option:hover,
.composer-bar__option.is-selected {
  background: color-mix(in srgb, var(--accent) 10%, transparent);
  color: var(--accent);
}

.composer-bar__option span,
.composer-bar__option small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.composer-bar__option span {
  font-size: 13px;
  font-weight: 620;
  line-height: 1.35;
}

.composer-bar__option small {
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.3;
}

.composer-bar__option--model {
  display: block;
  font-family: var(--font-mono);
  font-size: 12px;
}

.composer-bar__empty-option {
  padding: 10px;
  color: var(--text-muted);
  font-size: 12px;
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

/* ── 模型选择器 ── */
.composer-bar__model-select {
  flex: 1;
  min-width: 96px;
  max-width: 140px;
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

  .composer-bar__picker,
  .composer-bar__picker--llm {
    flex: 1 1 150px;
  }

  .composer-bar__picker-trigger {
    width: 100%;
    max-width: none;
  }

  .composer-bar__popover {
    max-width: calc(100vw - 32px);
  }

  .composer-bar__popover--llm {
    grid-template-columns: 1fr;
    min-width: min(320px, calc(100vw - 32px));
  }

  .composer-bar__model-column {
    border-left: 0;
    border-top: 1px solid color-mix(in srgb, var(--border-default) 72%, transparent);
    padding-top: 8px;
    padding-left: 0;
  }

  .composer-bar__actions {
    width: 100%;
    margin-left: 0;
    justify-content: flex-end;
  }

  .composer-bar__model-select {
    max-width: none;
  }
}
</style>
