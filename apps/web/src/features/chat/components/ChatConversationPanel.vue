<script setup lang="ts">
import type { BubbleRoleConfig, PromptProps } from "@opentiny/tiny-robot";

import type { ChatRobotMessage } from "../composables/useChatPresentation";
import ChatMessageStage from "./ChatMessageStage.vue";

interface Props {
  error: string;
  notice: string;
  threadLoading: boolean;
  robotMessages: ChatRobotMessage[];
  bubbleRoles: Record<string, BubbleRoleConfig>;
  starterPrompts: PromptProps[];
}

interface Emits {
  (e: "prompt", event: MouseEvent, item: PromptProps): void;
}

defineProps<Props>();
const emit = defineEmits<Emits>();

function handlePrompt(event: MouseEvent, item: PromptProps) {
  emit("prompt", event, item);
}
</script>

<template>
  <article class="chat-panel chat-panel--immersive">
    <div v-if="error" class="notice notice--danger">{{ error }}</div>
    <div v-else-if="notice" class="notice notice--info">{{ notice }}</div>
    <ChatMessageStage
      :loading="threadLoading"
      :messages="robotMessages"
      :bubble-roles="bubbleRoles"
      :starter-prompts="starterPrompts"
      @prompt="handlePrompt"
    />
  </article>
</template>

<style scoped>
.chat-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  min-height: 100%;
  --tr-bubble-list-padding: 0;
  --tr-bubble-list-gap: var(--space-4);
  --tr-bubble-box-bg: var(--surface-overlay);
  --tr-bubble-box-border: 1px solid var(--border-default);
  --tr-bubble-box-border-radius: var(--radius-lg);
  --tr-bubble-text-color: var(--text-primary);
  --tr-bubble-text-font-size: var(--text-sm);
  --tr-sender-bg-color: var(--surface-base);
  --tr-sender-text-color: var(--text-primary);
  --tr-sender-placeholder-color: var(--text-placeholder);
  --tr-prompt-bg: var(--surface-base);
  --tr-prompt-bg-hover: var(--accent-subtle);
  --tr-prompt-border-radius: var(--radius-lg);
  --tr-prompt-shadow: none;
  --tr-prompt-title-color: var(--text-primary);
  --tr-prompt-description-color: var(--text-muted);
}

.notice {
  padding: var(--space-3);
  border-radius: 18px;
  font-size: var(--text-sm);
  line-height: 1.5;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.35);
}

.notice--danger {
  background: color-mix(in srgb, var(--status-danger-bg) 80%, white);
  border: 1px solid color-mix(in srgb, var(--status-danger) 58%, white);
  color: var(--status-danger);
}

.notice--info {
  background: color-mix(in srgb, var(--accent-subtle) 72%, white);
  border: 1px solid color-mix(in srgb, var(--accent) 34%, var(--border-default));
  color: var(--text-secondary);
}
</style>
