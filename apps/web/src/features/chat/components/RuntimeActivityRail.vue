<script setup lang="ts">
import { computed, ref, watch } from "vue";

import type { ToolActivity } from "../composables/useChatThreadStream";
import type { ToolCallDetail } from "../composables/useChatPresentation";
import ToolCallDetailDialog from "./ToolCallDetailDialog.vue";
import {
  statusLabel,
  toolIcon,
  toolKindFromName,
  toolKindLabel,
} from "./toolCallDisplay";

const props = defineProps<{
  activities: ToolActivity[];
}>();

const activeTool = ref<ToolCallDetail | null>(null);
const collapsed = ref(false);

const visibleActivities = computed(() => props.activities);
const collapseLabel = computed(() => (collapsed.value ? "展开工具调用列表" : "折叠工具调用列表"));

watch(
  () => visibleActivities.value.length,
  (count) => {
    if (count === 0) collapsed.value = false;
  },
);

function activityKind(activity: ToolActivity): ToolCallDetail["kind"] {
  return activity.kind === "job" ? "job" : toolKindFromName(activity.name);
}

function openActivity(activity: ToolActivity) {
  const kind = activityKind(activity);
  activeTool.value = {
    id: activity.id,
    kind,
    name: activity.name,
    status: activity.status,
    inputPreview: activity.argumentsPreview,
    outputPreview: activity.resultPreview,
  };
}

function closeActivity() {
  activeTool.value = null;
}

function toggleCollapsed() {
  collapsed.value = !collapsed.value;
}
</script>

<template>
  <aside
    v-if="visibleActivities.length > 0"
    class="runtime-rail"
    :class="{ 'runtime-rail--collapsed': collapsed }"
    aria-label="当前运行活动"
  >
    <header class="runtime-rail__header">
      <div class="runtime-rail__summary">
        <p>当前运行</p>
        <span>{{ visibleActivities.length }}</span>
      </div>
      <button
        type="button"
        class="runtime-rail__toggle"
        :aria-expanded="!collapsed"
        aria-controls="runtime-rail-list"
        :aria-label="collapseLabel"
        :title="collapseLabel"
        @click="toggleCollapsed"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
          <path d="M7 10l5 5 5-5" />
        </svg>
      </button>
    </header>

    <div
      v-show="!collapsed"
      id="runtime-rail-list"
      class="runtime-rail__list"
      :aria-hidden="collapsed"
    >
      <button
        v-for="activity in visibleActivities"
        :key="activity.id"
        type="button"
        class="runtime-rail__item"
        :class="`runtime-rail__item--${activity.status}`"
        @click="openActivity(activity)"
      >
        <span class="runtime-rail__icon">{{ toolIcon(activityKind(activity)) }}</span>
        <span class="runtime-rail__body">
          <strong>{{ activity.name }}</strong>
          <small>{{ toolKindLabel(activityKind(activity)) }}</small>
        </span>
        <span class="runtime-rail__status" :class="`runtime-rail__status--${activity.status}`">
          {{ statusLabel(activity.status) }}
        </span>
      </button>
    </div>
  </aside>

  <ToolCallDetailDialog :tool="activeTool" @close="closeActivity" />
</template>

<style scoped>
.runtime-rail {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  gap: var(--space-3);
  width: 320px;
  max-height: calc(100vh - (var(--space-6) * 2) - var(--chat-dock-clearance, 212px));
  overflow: hidden;
  padding: var(--space-4);
  border: 1px solid color-mix(in srgb, var(--border-default) 78%, transparent);
  border-radius: 18px;
  background: color-mix(in srgb, var(--surface-base) 92%, transparent);
  box-shadow: var(--shadow-xs);
}

.runtime-rail--collapsed {
  grid-template-rows: auto;
  gap: 0;
  padding: var(--space-3);
}

.runtime-rail__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}

.runtime-rail__summary {
  display: inline-flex;
  min-width: 0;
  align-items: center;
  gap: var(--space-2);
}

.runtime-rail__summary p {
  margin: 0;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  white-space: nowrap;
}

.runtime-rail__summary span {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 22px;
  height: 22px;
  padding: 0 var(--space-2);
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 12%, white);
  color: var(--accent);
  font-size: var(--text-xs);
  font-weight: 700;
}

.runtime-rail__toggle {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: 1px solid color-mix(in srgb, var(--border-default) 80%, transparent);
  border-radius: 8px;
  background: color-mix(in srgb, var(--surface-base) 86%, transparent);
  color: var(--text-secondary);
  cursor: pointer;
  transition:
    border-color 0.18s ease,
    background-color 0.18s ease,
    color 0.18s ease;
}

.runtime-rail__toggle:hover {
  border-color: color-mix(in srgb, var(--accent) 38%, var(--border-default));
  background: color-mix(in srgb, var(--accent) 9%, transparent);
  color: var(--accent);
}

.runtime-rail__toggle svg {
  width: 16px;
  height: 16px;
  fill: none;
  stroke: currentColor;
  stroke-linecap: round;
  stroke-linejoin: round;
  stroke-width: 2.2;
  transition: transform 0.18s ease;
}

.runtime-rail--collapsed .runtime-rail__toggle svg {
  transform: rotate(180deg);
}

.runtime-rail__list {
  display: grid;
  min-height: 0;
  overflow: auto;
  gap: var(--space-2);
  padding-right: 2px;
}

.runtime-rail__item {
  display: grid;
  grid-template-columns: 30px minmax(0, 1fr);
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-3);
  border: 1px solid color-mix(in srgb, var(--border-default) 75%, transparent);
  border-radius: 14px;
  background: color-mix(in srgb, var(--surface-base) 88%, transparent);
  color: inherit;
  text-align: left;
  cursor: pointer;
  transition:
    border-color 0.18s ease,
    transform 0.18s ease,
    background-color 0.18s ease;
}

.runtime-rail__item:hover {
  border-color: color-mix(in srgb, var(--accent) 38%, var(--border-default));
  transform: translateY(-1px);
}

.runtime-rail__item--error {
  border-color: color-mix(in srgb, var(--status-danger) 45%, var(--border-default));
}

.runtime-rail__item--success {
  border-color: color-mix(in srgb, var(--status-success) 34%, var(--border-default));
}

.runtime-rail__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
  font-size: var(--text-xs);
  font-weight: 800;
}

.runtime-rail__body {
  display: grid;
  min-width: 0;
}

.runtime-rail__body strong {
  overflow: hidden;
  color: var(--text-primary);
  font-size: var(--text-sm);
  line-height: 1.45;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.runtime-rail__body small {
  color: var(--text-secondary);
  font-size: var(--text-xs);
  line-height: 1.5;
}

.runtime-rail__status {
  grid-column: 2;
  justify-self: start;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  margin-top: var(--space-1);
  padding: 3px 9px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--surface-muted) 92%, white);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 650;
  line-height: 1.4;
}

.runtime-rail__status--running {
  background: color-mix(in srgb, var(--accent) 12%, white);
  color: var(--accent);
}

.runtime-rail__status--success {
  background: color-mix(in srgb, var(--status-success) 12%, white);
  color: var(--status-success);
}

.runtime-rail__status--error {
  background: color-mix(in srgb, var(--status-danger) 12%, white);
  color: var(--status-danger);
}

@media (max-width: 1280px) {
  .runtime-rail {
    width: 100%;
    max-height: 240px;
  }
}
</style>
