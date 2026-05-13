<script setup lang="ts">
import { computed, ref } from "vue";

import {
  TURN_TIMELINE_CONTENT_TYPE,
  type ChatRobotMessage,
  type ToolCallDetail,
  type TurnTimelineContentItem,
} from "../composables/useChatPresentation";
import type { TurnTimelineItem } from "../composables/useChatThreadStream";
import ToolCallDetailDialog from "./ToolCallDetailDialog.vue";
import { statusLabel, toolIcon, toolKindLabel } from "./toolCallDisplay";

type ToolTimelineItem = Extract<TurnTimelineItem, { type: "tool_call" }>;
type TimelineEntry =
  | TurnTimelineItem
  | {
      type: "tool_group";
      id: string;
      tools: ToolTimelineItem[];
    };

const props = defineProps<{
  message: ChatRobotMessage;
  contentIndex: number;
}>();

const activeTool = ref<ToolCallDetail | null>(null);

const timelineItem = computed<TurnTimelineContentItem | null>(() => {
  if (!Array.isArray(props.message.content)) return null;
  const item = props.message.content[props.contentIndex];
  if (!item || typeof item !== "object") return null;
  return item as unknown as TurnTimelineContentItem;
});

const items = computed(() => {
  if (timelineItem.value?.type !== TURN_TIMELINE_CONTENT_TYPE) return [];
  return Array.isArray(timelineItem.value.items) ? timelineItem.value.items : [];
});

const timelineEntries = computed<TimelineEntry[]>(() => {
  const entries: TimelineEntry[] = [];
  let pendingTools: ToolTimelineItem[] = [];

  const flushPendingTools = () => {
    if (pendingTools.length === 0) return;
    if (pendingTools.length === 1) {
      entries.push(pendingTools[0]);
    } else {
      entries.push({
        type: "tool_group",
        id: `tool-group-${pendingTools[0].id}-${pendingTools.length}`,
        tools: pendingTools,
      });
    }
    pendingTools = [];
  };

  for (const item of items.value) {
    if (item.type === "tool_call") {
      pendingTools.push(item);
      continue;
    }
    flushPendingTools();
    entries.push(item);
  }
  flushPendingTools();

  return entries;
});

function openToolDetail(item: ToolTimelineItem) {
  const { id, kind, name, status, inputPreview, outputPreview } = item;
  activeTool.value = { id, kind, name, status, inputPreview, outputPreview };
}

function closeToolDetail() {
  activeTool.value = null;
}

function groupStatus(tools: ToolTimelineItem[]) {
  if (tools.some((tool) => tool.status === "error")) return "error";
  if (tools.some((tool) => tool.status === "running")) return "running";
  return "success";
}

function groupKindLabel(tools: ToolTimelineItem[]) {
  const firstKind = tools[0]?.kind;
  if (firstKind && tools.every((tool) => tool.kind === firstKind)) {
    return toolKindLabel(firstKind);
  }
  return "工具调用";
}

function toolPreview(tool: ToolTimelineItem) {
  return tool.inputPreview.trim() || toolKindLabel(tool.kind);
}
</script>

<template>
  <div class="turn-timeline" data-turn-timeline-content>
    <div
      v-for="item in timelineEntries"
      :key="item.id"
      class="turn-timeline__item"
      :class="`turn-timeline__item--${item.type}`"
    >
      <details v-if="item.type === 'reasoning'" class="turn-timeline__reasoning">
        <summary class="turn-timeline__reasoning-summary">
          <span class="turn-timeline__icon turn-timeline__icon--thinking">✧</span>
          <strong>思考</strong>
          <span class="turn-timeline__preview">{{ item.text }}</span>
          <span class="turn-timeline__chevron" aria-hidden="true">›</span>
        </summary>
        <div class="turn-timeline__reasoning-body">{{ item.text }}</div>
      </details>

      <details v-else-if="item.type === 'tool_group'" class="turn-timeline__tool-group">
        <summary class="turn-timeline__tool-group-summary">
          <span class="turn-timeline__icon turn-timeline__tool-icon">
            {{ toolIcon(item.tools[0]?.kind ?? "tool") }}
          </span>
          <strong>{{ groupKindLabel(item.tools) }} ×{{ item.tools.length }}，{{ statusLabel(groupStatus(item.tools)) }}</strong>
          <span class="turn-timeline__tool-group-preview">
            {{ item.tools.map((tool) => tool.name).join(" / ") }}
          </span>
          <span class="turn-timeline__chevron" aria-hidden="true">›</span>
        </summary>
        <div class="turn-timeline__tool-group-body">
          <button
            v-for="tool in item.tools"
            :key="tool.id"
            type="button"
            class="turn-timeline__tool turn-timeline__tool--grouped"
            :class="`turn-timeline__tool--${tool.status}`"
            @click="openToolDetail(tool)"
          >
            <span class="turn-timeline__icon turn-timeline__tool-icon">{{ toolIcon(tool.kind) }}</span>
            <span class="turn-timeline__tool-meta">
              <strong>{{ tool.name }}</strong>
              <small>{{ toolPreview(tool) }}</small>
            </span>
            <span class="turn-timeline__tool-status" :class="`turn-timeline__tool-status--${tool.status}`">
              {{ statusLabel(tool.status) }}
            </span>
          </button>
        </div>
      </details>

      <button
        v-else
        type="button"
        class="turn-timeline__tool"
        :class="`turn-timeline__tool--${item.status}`"
        @click="openToolDetail(item)"
      >
        <span class="turn-timeline__icon turn-timeline__tool-icon">{{ toolIcon(item.kind) }}</span>
        <span class="turn-timeline__tool-meta">
          <strong>{{ item.name }}</strong>
          <small>{{ toolKindLabel(item.kind) }}</small>
        </span>
        <span class="turn-timeline__tool-status" :class="`turn-timeline__tool-status--${item.status}`">
          {{ statusLabel(item.status) }}
        </span>
        <span class="turn-timeline__chevron" aria-hidden="true">›</span>
      </button>
    </div>
  </div>

  <ToolCallDetailDialog :tool="activeTool" @close="closeToolDetail" />
</template>

<style scoped>
.turn-timeline {
  display: grid;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.turn-timeline__item {
  min-width: 0;
}

.turn-timeline__reasoning {
  border: 1px solid color-mix(in srgb, var(--accent) 26%, var(--border-default));
  border-radius: var(--radius-lg);
  background: color-mix(in srgb, var(--accent-subtle) 58%, var(--surface-raised));
}

.turn-timeline__reasoning-summary,
.turn-timeline__tool-group-summary {
  display: grid;
  grid-template-columns: 22px auto minmax(0, 1fr) 18px;
  align-items: center;
  gap: var(--space-3);
  min-height: 38px;
  padding: 6px var(--space-3);
  color: var(--text-primary);
  cursor: pointer;
  font-size: var(--text-base);
  line-height: 1.4;
  list-style: none;
}

.turn-timeline__reasoning-summary::-webkit-details-marker,
.turn-timeline__tool-group-summary::-webkit-details-marker {
  display: none;
}

.turn-timeline__reasoning-summary strong,
.turn-timeline__tool-group-summary strong,
.turn-timeline__tool-meta strong {
  min-width: 0;
  color: var(--text-primary);
  font-size: var(--text-base);
  font-weight: 650;
  line-height: 1.4;
  white-space: nowrap;
}

.turn-timeline__preview,
.turn-timeline__tool-group-preview {
  min-width: 0;
  overflow: hidden;
  color: var(--text-secondary);
  font-size: var(--text-base);
  font-style: italic;
  line-height: 1.4;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.turn-timeline__chevron {
  color: var(--text-muted);
  font-size: 22px;
  line-height: 1;
  text-align: center;
  transition: transform 0.18s ease;
}

.turn-timeline__reasoning[open] .turn-timeline__chevron,
.turn-timeline__tool-group[open] .turn-timeline__chevron {
  transform: rotate(90deg);
}

.turn-timeline__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: var(--radius-full);
  flex: 0 0 auto;
  font-size: var(--text-xs);
  font-weight: 800;
}

.turn-timeline__icon--thinking {
  background: color-mix(in srgb, var(--accent) 10%, transparent);
  color: var(--accent);
}

.turn-timeline__reasoning-body {
  padding: 0 var(--space-4) var(--space-3) calc(var(--space-6) + var(--space-5));
  color: var(--text-secondary);
  font-size: var(--text-sm);
  line-height: 1.7;
  white-space: pre-wrap;
  word-break: break-word;
}

.turn-timeline__tool-group {
  overflow: hidden;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  background: color-mix(in srgb, var(--surface-overlay) 52%, var(--surface-raised));
}

.turn-timeline__tool-group-summary {
  border-bottom: 1px solid transparent;
  background: color-mix(in srgb, var(--surface-overlay) 72%, var(--surface-raised));
}

.turn-timeline__tool-group[open] .turn-timeline__tool-group-summary {
  border-bottom-color: var(--border-subtle);
}

.turn-timeline__tool-group-body {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3);
  background: color-mix(in srgb, var(--surface-raised) 82%, transparent);
}

.turn-timeline__tool {
  display: grid;
  grid-template-columns: 22px auto minmax(0, 1fr) auto 18px;
  align-items: center;
  gap: var(--space-3);
  width: 100%;
  min-height: 38px;
  padding: 6px var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  background: var(--surface-raised);
  color: inherit;
  text-align: left;
  cursor: pointer;
  transition:
    border-color 0.18s ease,
    transform 0.18s ease,
    background-color 0.18s ease;
}

.turn-timeline__tool:hover {
  border-color: color-mix(in srgb, var(--accent) 42%, var(--border-default));
  background: color-mix(in srgb, var(--surface-overlay) 62%, var(--surface-raised));
}

.turn-timeline__tool--grouped {
  grid-template-columns: 22px auto minmax(0, 1fr) auto;
  border-color: transparent;
  background: transparent;
}

.turn-timeline__tool--grouped:hover {
  border-color: var(--border-subtle);
}

.turn-timeline__tool--running {
  border-color: color-mix(in srgb, var(--accent) 26%, var(--border-default));
}

.turn-timeline__tool--error {
  border-color: color-mix(in srgb, var(--danger) 36%, var(--border-default));
  background: color-mix(in srgb, var(--danger-bg) 62%, var(--surface-raised));
}

.turn-timeline__tool--success {
  border-color: color-mix(in srgb, var(--success) 32%, var(--border-default));
  background: color-mix(in srgb, var(--success-bg) 56%, var(--surface-raised));
}

.turn-timeline__tool-icon {
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
}

.turn-timeline__tool--success .turn-timeline__tool-icon {
  background: var(--success-bg);
  color: var(--success);
}

.turn-timeline__tool--error .turn-timeline__tool-icon {
  background: var(--danger-bg);
  color: var(--danger);
}

.turn-timeline__tool-meta {
  display: contents;
  min-width: 0;
}

.turn-timeline__tool-meta small {
  min-width: 0;
  overflow: hidden;
  color: var(--text-secondary);
  font-size: var(--text-base);
  font-style: italic;
  line-height: 1.4;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.turn-timeline__tool-status {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 2px 8px;
  border-radius: var(--radius-full);
  background: color-mix(in srgb, var(--surface-overlay) 84%, transparent);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 600;
  line-height: 1.4;
  white-space: nowrap;
}

.turn-timeline__tool-status--running {
  background: var(--accent-subtle);
  color: var(--accent);
}

.turn-timeline__tool-status--success {
  background: var(--success-bg);
  color: var(--success);
}

.turn-timeline__tool-status--error {
  background: var(--danger-bg);
  color: var(--danger);
}
</style>
