<script setup lang="ts">
import type { ToolActivityStatus } from "../composables/useChatThreadStream";
import { previewText, statusLabel } from "./toolCallDisplay";

interface ToolDetailDialogPayload {
  name: string;
  status: ToolActivityStatus;
  inputPreview: string;
  outputPreview: string;
}

defineProps<{
  tool: ToolDetailDialogPayload | null;
}>();

defineEmits<{
  (e: "close"): void;
}>();
</script>

<template>
  <Teleport to="body">
    <div
      v-if="tool"
      class="tool-detail-dialog"
      role="dialog"
      aria-modal="true"
      aria-label="工具详情"
      @click.self="$emit('close')"
    >
      <div class="tool-detail-dialog__panel">
        <header class="tool-detail-dialog__header">
          <div class="tool-detail-dialog__title-wrap">
            <p class="tool-detail-dialog__eyebrow">工具详情</p>
            <strong>{{ tool.name }}</strong>
          </div>
          <div class="tool-detail-dialog__header-actions">
            <span class="tool-detail-dialog__status" :class="`tool-detail-dialog__status--${tool.status}`">
              {{ statusLabel(tool.status) }}
            </span>
            <button type="button" class="tool-detail-dialog__close" @click="$emit('close')">
              关闭
            </button>
          </div>
        </header>

        <section class="tool-detail-dialog__section">
          <div class="tool-detail-dialog__section-header">
            <span>输入</span>
          </div>
          <pre>{{ previewText(tool.inputPreview, "无输入参数") }}</pre>
        </section>

        <section class="tool-detail-dialog__section">
          <div class="tool-detail-dialog__section-header">
            <span>输出</span>
          </div>
          <pre>{{ previewText(tool.outputPreview, tool.status === "running" ? "等待工具返回…" : "无输出内容") }}</pre>
        </section>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.tool-detail-dialog {
  position: fixed;
  inset: 0;
  z-index: 1200;
  display: grid;
  place-items: center;
  padding: var(--space-6);
  background: rgba(15, 23, 42, 0.28);
  backdrop-filter: blur(8px);
}

.tool-detail-dialog__panel {
  width: min(760px, calc(100vw - 2 * var(--space-6)));
  max-height: min(720px, calc(100vh - 2 * var(--space-6)));
  overflow: auto;
  display: grid;
  gap: var(--space-4);
  padding: var(--space-5);
  border: 1px solid color-mix(in srgb, var(--border-default) 84%, transparent);
  border-radius: 24px;
  background:
    linear-gradient(
      135deg,
      color-mix(in srgb, var(--accent) 8%, transparent) 0%,
      transparent 56%
    ),
    color-mix(in srgb, var(--surface-base) 96%, white);
  box-shadow: 0 24px 64px rgba(15, 23, 42, 0.16);
}

.tool-detail-dialog__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
}

.tool-detail-dialog__title-wrap {
  display: grid;
  gap: 2px;
}

.tool-detail-dialog__title-wrap strong {
  color: var(--text-primary);
  font-size: var(--text-lg);
  line-height: 1.5;
}

.tool-detail-dialog__eyebrow {
  margin: 0;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.tool-detail-dialog__header-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.tool-detail-dialog__status {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 4px 10px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--surface-muted) 92%, white);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 600;
  line-height: 1.4;
  white-space: nowrap;
}

.tool-detail-dialog__status--running {
  background: color-mix(in srgb, var(--accent) 12%, white);
  color: var(--accent);
}

.tool-detail-dialog__status--success {
  background: color-mix(in srgb, var(--status-success) 12%, white);
  color: var(--status-success);
}

.tool-detail-dialog__status--error {
  background: color-mix(in srgb, var(--status-danger) 12%, white);
  color: var(--status-danger);
}

.tool-detail-dialog__close {
  border: 0;
  border-radius: 999px;
  padding: 6px 12px;
  background: color-mix(in srgb, var(--surface-muted) 90%, white);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 600;
  cursor: pointer;
}

.tool-detail-dialog__section {
  display: grid;
  gap: var(--space-2);
}

.tool-detail-dialog__section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.tool-detail-dialog__section pre {
  margin: 0;
  overflow: auto;
  padding: var(--space-4);
  border: 1px solid color-mix(in srgb, var(--border-default) 72%, transparent);
  border-radius: 18px;
  background: color-mix(in srgb, var(--surface-muted) 92%, white);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  line-height: 1.65;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
