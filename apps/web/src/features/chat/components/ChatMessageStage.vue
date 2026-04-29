<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";
import {
  type BubbleContentRendererMatch,
  type BubbleMessage,
  type ChatMessageContentItem,
  BubbleRenderers,
  TrBubbleList,
  TrBubbleProvider,
  TrPrompts,
  type BubbleRoleConfig,
  type PromptProps,
} from "@opentiny/tiny-robot";

import ToolCallSummaryContent from "./ToolCallSummaryContent.vue";
import {
  TOOL_SUMMARY_CONTENT_TYPE,
  type ChatRobotMessage,
} from "../composables/useChatPresentation";

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
const contentRendererMatches: BubbleContentRendererMatch[] = [
  {
    find: (_message: BubbleMessage, content: ChatMessageContentItem) =>
      content?.type === TOOL_SUMMARY_CONTENT_TYPE,
    renderer: ToolCallSummaryContent,
  },
];

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
  <div
    ref="stageRef"
    class="message-stage message-stage--flat message-stage--centered-assistant"
    @scroll.passive="handleStageScroll"
  >
    <div v-if="loading && messages.length === 0" class="empty-state">
      正在刷新消息…
    </div>
    <TrBubbleProvider
      v-else-if="messages.length > 0"
      class="message-stage__provider"
      :content-renderer-matches="contentRendererMatches"
      :fallback-content-renderer="BubbleRenderers.Markdown"
    >
      <TrBubbleList
        class="bubble-list"
        :messages="messages"
        :role-configs="bubbleRoles"
        content-render-mode="split"
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
  --assistant-readable-width: 680px;
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
  --tr-bubble-box-bg: transparent;
  --tr-bubble-box-border: none;
  --tr-bubble-box-shadow: none;
}

:deep(.tr-bubble[data-role="assistant"]) {
  display: flex;
  justify-content: center;
}

:deep(.tr-bubble[data-role="assistant"] .tr-bubble__body) {
  width: min(100%, calc(var(--assistant-readable-width) + 44px));
  justify-content: center;
}

:deep(.tr-bubble[data-role="assistant"] .tr-bubble__content) {
  flex: 0 1 var(--assistant-readable-width);
  width: min(100%, var(--assistant-readable-width));
  max-width: var(--assistant-readable-width);
  min-width: 0;
}

:deep(.tr-bubble__box[data-role="assistant"]) {
  width: 100%;
  max-width: none !important;
  background: transparent !important;
  border: none !important;
  box-shadow: none !important;
  padding: 0 !important;
}

:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__reasoning),
:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown),
:deep(.tr-bubble__box[data-role="assistant"] .header),
:deep(.tr-bubble__box[data-role="assistant"] .detail),
:deep(.tr-bubble__box[data-role="assistant"] .detail-content) {
  width: 100%;
  max-width: 100%;
}

:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__reasoning) {
  margin-bottom: var(--space-3);
}

:deep(.tr-bubble__box[data-role="assistant"] .header) {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  margin-bottom: var(--space-3);
  color: var(--text-secondary);
  font-size: var(--text-base);
  line-height: 1.7;
}

:deep(.tr-bubble__box[data-role="assistant"] .icon-and-text) {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: 4px 12px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--surface-muted) 92%, white);
  color: var(--text-secondary);
}

:deep(.tr-bubble__box[data-role="assistant"] .icon-and-text.thinking) {
  background: color-mix(in srgb, var(--accent) 12%, white);
  color: var(--accent);
}

:deep(.tr-bubble__box[data-role="assistant"] .title::before) {
  content: "状态 · ";
  color: inherit;
  font-weight: 500;
}

:deep(.tr-bubble__box[data-role="assistant"] .title) {
  font-size: var(--text-base);
  font-weight: 600;
  line-height: 1.7;
  color: inherit;
}

:deep(.tr-bubble__box[data-role="assistant"] .detail) {
  display: block;
  border: 1px solid color-mix(in srgb, var(--border-default) 78%, transparent);
  border-radius: 14px;
  padding: var(--space-3) var(--space-4);
  background:
    linear-gradient(
      135deg,
      color-mix(in srgb, var(--accent) 12%, transparent) 0%,
      color-mix(in srgb, var(--accent) 5%, transparent) 34%,
      transparent 100%
    ),
    color-mix(in srgb, var(--surface-base) 80%, transparent);
}

:deep(.tr-bubble__box[data-role="assistant"] .side-border) {
  display: none;
}

:deep(.tr-bubble__box[data-role="assistant"] .detail-content),
:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown),
:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown p),
:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown li) {
  color: var(--text-primary);
  font-size: var(--text-base);
  line-height: 1.85;
}

:deep(.tr-bubble__box[data-role="assistant"] .detail-content),
:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown p),
:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown ul),
:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown ol) {
  margin: 0 0 var(--space-3);
}

:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__markdown > :last-child),
:deep(.tr-bubble__box[data-role="assistant"] .detail-content:last-child) {
  margin-bottom: 0;
}

:deep(.tr-bubble__box[data-role="assistant"] .tr-bubble__after) {
  display: none;
}

:deep(.tr-bubble-item) {
  margin-bottom: var(--space-4);
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
