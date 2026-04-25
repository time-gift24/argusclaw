<script setup lang="ts">
import { TrBubbleList, TrPrompts, type BubbleRoleConfig, type PromptProps } from "@opentiny/tiny-robot";

import type { ChatRobotMessage } from "../composables/useChatPresentation";

interface Props {
  loading: boolean;
  messages: ChatRobotMessage[];
  bubbleRoles: Record<string, BubbleRoleConfig>;
  starterPrompts: PromptProps[];
}

interface Emits {
  (e: "prompt", event: MouseEvent, item: PromptProps): void;
}

defineProps<Props>();
const emit = defineEmits<Emits>();

function handlePromptClick(event: MouseEvent, item: PromptProps) {
  emit("prompt", event, item);
}
</script>

<template>
  <div class="message-stage">
    <div v-if="loading && messages.length === 0" class="empty-state">
      正在刷新消息…
    </div>
    <TrBubbleList
      v-else-if="messages.length > 0"
      class="bubble-list"
      :messages="messages"
      :role-configs="bubbleRoles"
      auto-scroll
      group-strategy="divider"
    />
    <div v-else class="prompt-panel">
      <p class="prompt-title">快速开始</p>
      <TrPrompts :items="starterPrompts" wrap @item-click="handlePromptClick" />
    </div>
  </div>
</template>

<style scoped>
.message-stage {
  flex: 1;
  min-height: 500px;
  max-height: 650px;
  overflow: auto;
  padding: var(--space-4);
  background:
    radial-gradient(circle at top left, var(--accent-subtle), transparent 34%),
    var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
}

.bubble-list {
  min-height: 100%;
}

.prompt-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.prompt-title {
  margin: 0;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-weight: 590;
}

.empty-state {
  display: grid;
  min-height: 128px;
  place-items: center;
  color: var(--text-muted);
  font-size: var(--text-sm);
  line-height: 1.6;
  text-align: center;
}

:deep([data-role="user"]) {
  --tr-bubble-box-bg: var(--accent-subtle);
  --tr-bubble-box-border: 1px solid rgba(94, 106, 210, 0.2);
}

:deep(.chat-avatar),
:deep(.prompt-icon) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  height: 28px;
  padding: 0 var(--space-1);
  border-radius: var(--radius-full);
  background: var(--accent-subtle);
  color: var(--accent);
  font-size: var(--text-xs);
  font-weight: 700;
}

:deep(.chat-avatar--user) {
  background: var(--status-success-bg);
  color: var(--status-success);
}

:deep(.chat-avatar--tool) {
  background: var(--status-warning-bg);
  color: var(--status-warning);
}

@media (max-width: 1180px) {
  .message-stage {
    min-height: 420px;
    max-height: none;
  }
}
</style>
