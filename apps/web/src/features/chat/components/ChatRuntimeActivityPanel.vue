<script setup lang="ts">
import { TinyTag } from "@/lib/opentiny";
import type { ToolActivity, ToolActivityStatus } from "../composables/useChatThreadStream";

interface Props {
  notice: string;
  activities: ToolActivity[];
}

defineProps<Props>();

function runtimeActivityStatusLabel(status: ToolActivityStatus) {
  if (status === "success") return "完成";
  if (status === "error") return "失败";
  return "运行中";
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
      <article
        v-for="activity in activities"
        :key="activity.id"
        class="tool-activity-card"
        :class="`tool-activity-card--${activity.status}`"
      >
        <div class="tool-activity-card__header">
          <strong>{{ activity.name }}</strong>
          <span>{{ runtimeActivityStatusLabel(activity.status) }}</span>
        </div>
        <pre v-if="activity.argumentsPreview">{{ activity.argumentsPreview }}</pre>
        <pre v-if="activity.resultPreview">{{ activity.resultPreview }}</pre>
      </article>
    </div>
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
  gap: var(--space-2);
  padding: var(--space-3);
  background: rgba(255, 255, 255, 0.72);
  border: 1px solid rgba(148, 163, 184, 0.16);
  border-radius: 18px;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.6);
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

.tool-activity-card__header span {
  color: var(--text-muted);
  font-size: var(--text-xs);
  font-weight: 590;
}

.tool-activity-card pre {
  max-height: 160px;
  margin: 0;
  overflow: auto;
  color: var(--text-secondary);
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  line-height: 1.5;
  white-space: pre-wrap;
}
</style>
