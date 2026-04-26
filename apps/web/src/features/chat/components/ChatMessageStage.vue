<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";
import {
  BubbleRenderers,
  TrBubbleList,
  TrBubbleProvider,
  TrPrompts,
  type BubbleRoleConfig,
  type PromptProps,
} from "@opentiny/tiny-robot";

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

const props = defineProps<Props>();
const emit = defineEmits<Emits>();
const stageRef = ref<HTMLDivElement | null>(null);
const shouldStickToBottom = ref(true);
const AUTO_SCROLL_THRESHOLD = 72;

function handlePromptClick(event: MouseEvent, item: PromptProps) {
  emit("prompt", event, item);
}

function getDistanceFromBottom(element: HTMLDivElement) {
  return element.scrollHeight - element.clientHeight - element.scrollTop;
}

function updateStickToBottom() {
  const stage = stageRef.value;
  if (!stage) return;
  shouldStickToBottom.value = getDistanceFromBottom(stage) <= AUTO_SCROLL_THRESHOLD;
}

function scrollStageToBottom() {
  const stage = stageRef.value;
  if (!stage) return;

  const top = stage.scrollHeight;
  if (typeof stage.scrollTo === "function") {
    stage.scrollTo({ top, behavior: "auto" });
  } else {
    stage.scrollTop = top;
  }
}

function handleStageScroll() {
  updateStickToBottom();
}

watch(
  () => props.messages,
  async (messages) => {
    if (messages.length === 0 || !shouldStickToBottom.value) return;
    await nextTick();
    scrollStageToBottom();
  },
  { deep: true },
);

onMounted(async () => {
  await nextTick();
  updateStickToBottom();
  if (props.messages.length > 0) {
    scrollStageToBottom();
  }
});
</script>

<template>
  <div ref="stageRef" class="message-stage message-stage--flat" @scroll.passive="handleStageScroll">
    <div v-if="loading && messages.length === 0" class="empty-state">
      正在刷新消息…
    </div>
    <TrBubbleProvider
      v-else-if="messages.length > 0"
      class="message-stage__provider"
      :fallback-content-renderer="BubbleRenderers.Markdown"
    >
      <TrBubbleList
        class="bubble-list"
        :messages="messages"
        :role-configs="bubbleRoles"
        auto-scroll
        group-strategy="divider"
      />
    </TrBubbleProvider>
    <div v-else class="prompt-panel">
      <p class="prompt-title">快速开始</p>
      <TrPrompts :items="starterPrompts" wrap @item-click="handlePromptClick" />
    </div>
  </div>
</template>

<style scoped>
.message-stage {
  flex: 1;
  min-height: 0;
  height: 100%;
  overflow: auto;
  padding: var(--space-2) 0 calc(var(--chat-dock-clearance, 212px) + var(--space-5));
  overscroll-behavior: contain;
}

.message-stage--flat {
  background: transparent;
  border: 0;
  border-radius: 0;
  box-shadow: none;
}

.message-stage__provider {
  display: block;
  min-height: 100%;
}

.bubble-list {
  min-height: 100%;
}

.prompt-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-2) 0;
}

.prompt-title {
  margin: 0;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-weight: 590;
  letter-spacing: 0.08em;
  text-transform: uppercase;
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
  --tr-bubble-box-bg: rgba(94, 106, 210, 0.1);
  --tr-bubble-box-border: 1px solid rgba(94, 106, 210, 0.22);
}

:deep([data-role="assistant"]) {
  --tr-bubble-box-bg: rgba(255, 255, 255, 0.92);
  --tr-bubble-box-border: 1px solid rgba(148, 163, 184, 0.2);
  --tr-bubble-box-shadow: 0 12px 28px rgba(15, 23, 42, 0.06);
}

:deep(.tr-bubble-item) {
  margin-bottom: var(--space-4);
}

:deep(.tr-bubble-reasoning) {
  margin-bottom: var(--space-3);
}

:deep(.tr-bubble-reasoning__trigger) {
  border-radius: 14px;
  background: rgba(148, 163, 184, 0.12);
}

:deep(.tr-bubble-reasoning__content) {
  border-left: 2px solid rgba(94, 106, 210, 0.18);
  padding-left: var(--space-3);
  color: var(--text-muted);
}

:deep(.chat-avatar),
:deep(.prompt-icon) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  height: 30px;
  padding: 0 var(--space-2);
  border-radius: var(--radius-full);
  background: rgba(94, 106, 210, 0.12);
  color: var(--accent);
  font-size: var(--text-xs);
  font-weight: 700;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.6);
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
    padding-bottom: calc(var(--chat-dock-clearance, 228px) + var(--space-4));
  }
}
</style>
