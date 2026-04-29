<script setup lang="ts">
import { computed, ref } from "vue";

import { TinyTag } from "@/lib/opentiny";
import type { ToolActivity } from "../composables/useChatThreadStream";
import ToolCallDetailDialog from "./ToolCallDetailDialog.vue";
import {
  statusLabel,
  toolIcon,
  toolKindFromName,
  toolKindLabel,
} from "./toolCallDisplay";

interface Props {
  notice: string;
  activities: ToolActivity[];
}

const props = defineProps<Props>();
const activeActivityId = ref<string>("");

const activeActivity = computed(() =>
  props.activities.find((activity) => activity.id === activeActivityId.value) ?? null);

function openActivity(activity: ToolActivity) {
  activeActivityId.value = activity.id;
}

function closeActivityDetail() {
  activeActivityId.value = "";
}
</script>

<template>
  <div v-if="notice || activities.length > 0" class="runtime-activity-panel">
    <div class="runtime-activity-header">
      <div>
        <p class="eyebrow">Runtime</p>
        <strong>本轮运行活动</strong>
      </div>
      <TinyTag v-if="activities.length > 0" type="info">
        {{ activities.length }} 项
      </TinyTag>
    </div>
    <p v-if="notice" class="runtime-notice">{{ notice }}</p>
    <div v-if="activities.length > 0" class="tool-activity-list">
      <button
        v-for="activity in activities"
        :key="activity.id"
        class="tool-activity-card"
        :class="`tool-activity-card--${activity.status}`"
        type="button"
        @click="openActivity(activity)"
      >
        <span class="tool-activity-card__icon">{{ toolIcon(toolKindFromName(activity.name)) }}</span>
        <div class="tool-activity-card__header">
          <div class="tool-activity-card__meta">
            <strong>{{ activity.name }}</strong>
            <small>{{ toolKindLabel(toolKindFromName(activity.name)) }} · 点击查看输入/输出</small>
          </div>
          <span class="tool-activity-card__status" :class="`tool-activity-card__status--${activity.status}`">
            {{ statusLabel(activity.status) }}
          </span>
        </div>
      </button>
    </div>
    <ToolCallDetailDialog
      :tool="activeActivity ? {
        name: activeActivity.name,
        status: activeActivity.status,
        inputPreview: activeActivity.argumentsPreview,
        outputPreview: activeActivity.resultPreview,
      } : null"
      @close="closeActivityDetail"
    />
  </div>
</template>

<style scoped>
.runtime-activity-panel {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-4);
  background: rgba(255, 255, 255, 0.76);
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 24px;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.6),
    0 16px 32px rgba(15, 23, 42, 0.04);
}

.runtime-activity-header,
.tool-activity-card__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-3);
}

.runtime-activity-header strong,
.tool-activity-card__header strong {
  color: var(--text-primary);
  font-size: var(--text-sm);
}

.runtime-notice {
  margin: 0;
  color: var(--warning);
  font-size: var(--text-sm);
  line-height: 1.5;
  padding: var(--space-2) var(--space-3);
  border-radius: 16px;
  background: rgba(245, 158, 11, 0.08);
}

.tool-activity-list {
  display: grid;
  gap: var(--space-2);
}

.tool-activity-card {
  display: grid;
  grid-template-columns: 28px minmax(0, 1fr);
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3);
  background: rgba(255, 255, 255, 0.72);
  border: 1px solid rgba(148, 163, 184, 0.16);
  border-radius: 18px;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.6);
  text-align: left;
  cursor: pointer;
}

.tool-activity-card--running {
  border-color: rgba(94, 106, 210, 0.35);
}

.tool-activity-card--success {
  border-color: var(--status-success);
}

.tool-activity-card--error {
  border-color: var(--status-danger);
}

.tool-activity-card__icon {
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

.tool-activity-card__meta {
  display: grid;
  gap: 2px;
  min-width: 0;
}

.tool-activity-card__meta small {
  color: var(--text-secondary);
  font-size: var(--text-xs);
  line-height: 1.5;
}

.tool-activity-card__status {
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

.tool-activity-card__status--running {
  background: color-mix(in srgb, var(--accent) 12%, white);
  color: var(--accent);
}

.tool-activity-card__status--success {
  background: color-mix(in srgb, var(--status-success) 12%, white);
  color: var(--status-success);
}

.tool-activity-card__status--error {
  background: color-mix(in srgb, var(--status-danger) 12%, white);
  color: var(--status-danger);
}
</style>
