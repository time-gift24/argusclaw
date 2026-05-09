<script setup lang="ts">
import { computed, ref } from "vue";

import {
  TOOL_SUMMARY_CONTENT_TYPE,
  type ChatRobotMessage,
  type ToolCallDetail,
  type ToolSummaryContentItem,
} from "../composables/useChatPresentation";
import ToolCallDetailDialog from "./ToolCallDetailDialog.vue";
import {
  statusLabel,
  toolIcon,
  toolKindLabel,
} from "./toolCallDisplay";

const props = defineProps<{
  message: ChatRobotMessage;
  contentIndex: number;
}>();

const activeTool = ref<ToolCallDetail | null>(null);

const summaryItem = computed<ToolSummaryContentItem | null>(() => {
  if (!Array.isArray(props.message.content)) return null;
  const item = props.message.content[props.contentIndex];
  if (!item || typeof item !== "object") return null;
  return item as unknown as ToolSummaryContentItem;
});

const toolDetails = computed(() => {
  if (summaryItem.value?.type !== TOOL_SUMMARY_CONTENT_TYPE) return [];
  return Array.isArray(summaryItem.value.toolDetails) ? summaryItem.value.toolDetails : [];
});

function openToolDetail(tool: ToolCallDetail) {
  activeTool.value = tool;
}

function closeToolDetail() {
  activeTool.value = null;
}
</script>

<template>
  <div class="tool-summary" data-tool-summary-content>
    <button
      v-for="tool in toolDetails"
      :key="tool.id"
      type="button"
      class="tool-summary__row"
      :class="`tool-summary__row--${tool.status}`"
      @click="openToolDetail(tool)"
    >
      <span class="tool-summary__icon">{{ toolIcon(tool.kind) }}</span>
      <span class="tool-summary__meta">
        <strong>{{ tool.name }}</strong>
        <small>{{ toolKindLabel(tool.kind) }}</small>
      </span>
      <span class="tool-summary__status" :class="`tool-summary__status--${tool.status}`">
        {{ statusLabel(tool.status) }}
      </span>
    </button>
  </div>

  <ToolCallDetailDialog :tool="activeTool" @close="closeToolDetail" />
</template>

<style scoped>
.tool-summary {
  display: grid;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.tool-summary__row {
  display: grid;
  grid-template-columns: 28px minmax(0, 1fr) auto;
  align-items: center;
  gap: var(--space-3);
  width: 100%;
  padding: var(--space-3) var(--space-4);
  border: 1px solid color-mix(in srgb, var(--border-default) 75%, transparent);
  border-radius: 16px;
  background:
    linear-gradient(
      135deg,
      color-mix(in srgb, var(--accent) 8%, transparent) 0%,
      transparent 72%
    ),
    color-mix(in srgb, var(--surface-base) 88%, transparent);
  text-align: left;
  cursor: pointer;
  transition:
    border-color 0.18s ease,
    transform 0.18s ease,
    background-color 0.18s ease;
}

.tool-summary__row:hover {
  border-color: color-mix(in srgb, var(--accent) 38%, var(--border-default));
  transform: translateY(-1px);
}

.tool-summary__row--error {
  border-color: color-mix(in srgb, var(--status-danger) 45%, var(--border-default));
}

.tool-summary__row--success {
  border-color: color-mix(in srgb, var(--status-success) 34%, var(--border-default));
}

.tool-summary__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
  font-size: 13px;
  font-weight: 700;
}

.tool-summary__meta {
  display: grid;
  min-width: 0;
}

.tool-summary__meta strong {
  color: var(--text-primary);
  font-size: var(--text-sm);
  line-height: 1.5;
}

.tool-summary__meta small {
  color: var(--text-secondary);
  font-size: var(--text-xs);
  line-height: 1.5;
}

.tool-summary__status {
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

.tool-summary__status--running {
  background: color-mix(in srgb, var(--accent) 12%, white);
  color: var(--accent);
}

.tool-summary__status--success {
  background: color-mix(in srgb, var(--status-success) 12%, white);
  color: var(--status-success);
}

.tool-summary__status--error {
  background: color-mix(in srgb, var(--status-danger) 12%, white);
  color: var(--status-danger);
}

</style>
