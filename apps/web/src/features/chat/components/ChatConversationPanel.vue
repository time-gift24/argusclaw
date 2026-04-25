<script setup lang="ts">
import type { BubbleRoleConfig, PromptProps } from "@opentiny/tiny-robot";

import { TinyButton } from "@/lib/opentiny";
import type { ToolActivity } from "../composables/useChatThreadStream";
import type { ChatRobotMessage } from "../composables/useChatPresentation";
import ChatMessageStage from "./ChatMessageStage.vue";
import ChatRuntimeActivityPanel from "./ChatRuntimeActivityPanel.vue";

interface Props {
  title: string;
  modelLabel: string;
  providerName: string | null;
  hasActiveThread: boolean;
  error: string;
  actionMessage: string;
  runtimeNotice: string;
  runtimeActivities: ToolActivity[];
  threadLoading: boolean;
  robotMessages: ChatRobotMessage[];
  bubbleRoles: Record<string, BubbleRoleConfig>;
  starterPrompts: PromptProps[];
}

interface Emits {
  (e: "refresh"): void;
  (e: "activate"): void;
  (e: "cancel"): void;
  (e: "prompt", event: MouseEvent, item: PromptProps): void;
}

defineProps<Props>();
const emit = defineEmits<Emits>();

function handlePrompt(event: MouseEvent, item: PromptProps) {
  emit("prompt", event, item);
}
</script>

<template>
  <article class="chat-panel shell-card">
    <header class="chat-panel__header">
      <div>
        <p class="eyebrow">Conversation</p>
        <h3 class="section-heading">{{ title }}</h3>
        <p class="section-copy">
          {{ modelLabel }}
          <span v-if="providerName"> · {{ providerName }}</span>
        </p>
      </div>
      <div class="chat-actions">
        <TinyButton :disabled="!hasActiveThread" @click="emit('refresh')">刷新</TinyButton>
        <TinyButton :disabled="!hasActiveThread" @click="emit('activate')">激活</TinyButton>
        <TinyButton data-testid="cancel-thread" :disabled="!hasActiveThread" @click="emit('cancel')">取消运行</TinyButton>
      </div>
    </header>

    <div v-if="error" class="notice notice--danger">{{ error }}</div>
    <div v-if="actionMessage" class="notice notice--success">{{ actionMessage }}</div>

    <ChatRuntimeActivityPanel
      :notice="runtimeNotice"
      :activities="runtimeActivities"
    />

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
  gap: var(--space-4);
  min-height: 0;
  padding: var(--space-5);
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

.chat-panel__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
}

.notice {
  padding: var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  line-height: 1.5;
}

.notice--danger {
  background: var(--status-danger-bg);
  border: 1px solid var(--status-danger);
  color: var(--status-danger);
}

.notice--success {
  background: var(--status-success-bg);
  border: 1px solid var(--status-success);
  color: var(--status-success);
}
</style>
