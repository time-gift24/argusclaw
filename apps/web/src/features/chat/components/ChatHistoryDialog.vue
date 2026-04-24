<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { TinyButton, TinyInput } from "@/lib/opentiny";
import { getApiClient, type ChatSessionSummary, type ChatThreadSummary } from "@/lib/api";
import { formatSessionName, formatThreadTitle } from "../composables/useChatSessions";

interface Props {
  modelValue: boolean;
  sessions: ChatSessionSummary[];
  activeSessionId: string;
  activeThreadId: string;
  sessionListLoading: boolean;
}

interface Emits {
  (e: "update:modelValue", value: boolean): void;
  (e: "selectThread", sessionId: string, threadId: string): void;
  (e: "deleteSession", sessionId: string): void;
  (e: "renameSession", sessionId: string, name: string): void;
}

const props = defineProps<Props>();
const emit = defineEmits<Emits>();

const selectedSessionId = ref<string | null>(null);
const sessionThreads = ref<ChatThreadSummary[]>([]);
const threadsLoading = ref(false);
const deleteConfirmSessionId = ref<string | null>(null);
const renameSessionId = ref<string | null>(null);
const renameValue = ref("");
const renameSaving = ref(false);
const hasMounted = ref(false);

const selectedSession = computed(() =>
  props.sessions.find((s) => s.id === selectedSessionId.value) ?? null,
);

function formatRelativeTime(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMs / 3600000);
  const diffDays = Math.floor(diffMs / 86400000);

  if (diffMins < 1) return "刚刚";
  if (diffMins < 60) return `${diffMins} 分钟前`;
  if (diffHours < 24) return `${diffHours} 小时前`;
  if (diffDays < 7) return `${diffDays} 天前`;
  return date.toLocaleDateString("zh-CN", { month: "short", day: "numeric" });
}

function renderTimestamp(dateStr: string) {
  return hasMounted.value ? formatRelativeTime(dateStr) : dateStr.slice(0, 16).replace("T", " ");
}

async function handleSelectSession(sessionId: string) {
  const api = getApiClient();
  selectedSessionId.value = sessionId;
  sessionThreads.value = [];
  threadsLoading.value = true;
  try {
    sessionThreads.value = await api.listChatThreads!(sessionId);
  } catch {
    sessionThreads.value = [];
  } finally {
    threadsLoading.value = false;
  }
}

function handleSelectThread(sessionId: string, threadId: string) {
  emit("update:modelValue", false);
  emit("selectThread", sessionId, threadId);
}

function handleDeleteClick(sessionId: string, event: Event) {
  event.stopPropagation();
  deleteConfirmSessionId.value = sessionId;
}

function handleDeleteConfirm(sessionId: string) {
  emit("deleteSession", sessionId);
  deleteConfirmSessionId.value = null;
}

function handleRenameClick(session: ChatSessionSummary, event: Event) {
  event.stopPropagation();
  renameSessionId.value = session.id;
  renameValue.value = session.name;
}

function handleRenameSubmit() {
  if (!renameSessionId.value) return;
  renameSaving.value = true;
  emit("renameSession", renameSessionId.value, renameValue.value.trim());
  renameSaving.value = false;
  renameSessionId.value = null;
  renameValue.value = "";
}

watch(
  () => props.modelValue,
  async (nextOpen) => {
    if (nextOpen) {
      hasMounted.value = true;
      const preferred = props.activeSessionId ?? props.sessions[0]?.id ?? null;
      deleteConfirmSessionId.value = null;
      renameSessionId.value = null;
      if (preferred) {
        await handleSelectSession(preferred);
      }
    }
  },
);

watch(
  () => props.activeSessionId,
  async (nextId) => {
    if (props.modelValue && nextId && nextId !== selectedSessionId.value) {
      await handleSelectSession(nextId);
    }
  },
);

function closeDialog() {
  emit("update:modelValue", false);
}
</script>

<template>
  <Teleport to="body">
    <dialog
      v-if="modelValue"
      class="history-dialog"
      @close="closeDialog"
      @click.self="closeDialog"
    >
      <div class="history-dialog__panel" @click.stop>
        <header class="history-dialog__header">
          <div class="history-dialog__title-row">
            <div class="history-dialog__title-icon">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
            </div>
            <div>
              <h2 class="history-dialog__title">历史会话</h2>
              <p class="history-dialog__subtitle">会话历史</p>
            </div>
          </div>
          <TinyButton size="small" @click="closeDialog">关闭</TinyButton>
        </header>

        <div class="history-dialog__body">
          <!-- Session list column -->
          <div class="history-dialog__col history-dialog__col--sessions">
            <div class="history-dialog__col-header">会话列表</div>
            <div class="history-dialog__col-content">
              <div v-if="sessionListLoading" class="history-dialog__loading">
                加载中…
              </div>
              <div v-else-if="sessions.length === 0" class="history-dialog__empty">
                暂无历史会话
              </div>
              <div v-else class="history-dialog__list">
                <div
                  v-for="session in sessions"
                  :key="session.id"
                  class="history-dialog__session-item"
                  :class="{ active: session.id === selectedSessionId }"
                  @click="handleSelectSession(session.id)"
                  @dblclick="handleRenameClick(session, $event)"
                >
                  <div class="history-dialog__session-info">
                    <span class="history-dialog__session-name">
                      {{ formatSessionName(session) }}
                    </span>
                    <span v-if="session.id === activeSessionId" class="history-dialog__active-dot">✓</span>
                  </div>
                  <div class="history-dialog__session-meta">
                    <span>{{ session.thread_count }} 个对话</span>
                    <span>{{ renderTimestamp(session.updated_at) }}</span>
                  </div>

                  <!-- Delete confirm -->
                  <div v-if="deleteConfirmSessionId === session.id" class="history-dialog__inline-actions">
                    <TinyButton size="small" @click.stop="handleDeleteConfirm(session.id)">删除</TinyButton>
                    <TinyButton size="small" @click.stop="deleteConfirmSessionId = null">取消</TinyButton>
                  </div>
                  <div v-else class="history-dialog__item-actions">
                    <button class="history-dialog__action-btn" title="重命名" @click.stop="handleRenameClick(session, $event)">✎</button>
                    <button class="history-dialog__action-btn history-dialog__action-btn--danger" title="删除" @click.stop="handleDeleteClick(session.id, $event)">✕</button>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- Thread list column -->
          <div class="history-dialog__col history-dialog__col--threads">
            <div class="history-dialog__col-header">
              <span>对话列表</span>
              <span class="history-dialog__col-subtitle">
                {{ selectedSession ? formatSessionName(selectedSession) : "请选择左侧会话" }}
              </span>
            </div>
            <div class="history-dialog__col-content">
              <div v-if="!selectedSessionId" class="history-dialog__empty">
                请选择左侧会话
              </div>
              <div v-else-if="threadsLoading" class="history-dialog__loading">
                加载中…
              </div>
              <div v-else-if="sessionThreads.length === 0" class="history-dialog__empty">
                暂无对话
              </div>
              <div v-else class="history-dialog__list">
                <div
                  v-for="thread in sessionThreads"
                  :key="thread.id"
                  class="history-dialog__session-item"
                  :class="{ active: thread.id === activeThreadId && selectedSessionId === activeSessionId }"
                  @click="handleSelectThread(selectedSessionId!, thread.id)"
                >
                  <div class="history-dialog__session-info">
                    <span class="history-dialog__session-name">
                      {{ formatThreadTitle(thread) }}
                    </span>
                    <span v-if="thread.id === activeThreadId && selectedSessionId === activeSessionId" class="history-dialog__active-dot">✓</span>
                  </div>
                  <div class="history-dialog__session-meta">
                    <span>{{ thread.turn_count }} turns</span>
                    <span>{{ renderTimestamp(thread.updated_at) }}</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <footer class="history-dialog__footer">
          {{ sessions.length }} 个会话 · 双击会话名重命名
        </footer>

        <!-- Rename dialog -->
        <div v-if="renameSessionId" class="history-dialog__rename-overlay" @click.self="renameSessionId = null">
          <div class="history-dialog__rename-box">
            <h3>重命名会话</h3>
            <p>留空即可恢复为 ID 回退显示。</p>
            <TinyInput
              v-model="renameValue"
              placeholder="输入会话名称"
              autofocus
              @keydown.enter="handleRenameSubmit"
              @keydown.esc="renameSessionId = null"
            />
            <div class="history-dialog__rename-actions">
              <TinyButton size="small" @click="renameSessionId = null">取消</TinyButton>
              <TinyButton size="small" type="primary" :loading="renameSaving" @click="handleRenameSubmit">保存</TinyButton>
            </div>
          </div>
        </div>
      </div>
    </dialog>
  </Teleport>
</template>

<style scoped>
.history-dialog {
  position: fixed;
  inset: 0;
  z-index: 1000;
  width: 100%;
  height: 100%;
  max-width: 100%;
  max-height: 100%;
  padding: 0;
  border: none;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
}

.history-dialog::backdrop {
  background: rgba(0, 0, 0, 0.4);
  backdrop-filter: blur(2px);
}

.history-dialog__panel {
  position: relative;
  width: min(920px, 95vw);
  max-height: 85vh;
  background: var(--surface-base, #fff);
  border: 1px solid var(--border-default, #e2e5eb);
  border-radius: var(--radius-lg, 8px);
  box-shadow: var(--shadow-lg, 0 10px 15px -3px rgba(0, 0, 0, 0.08));
  display: flex;
  flex-direction: column;
  overflow: hidden;
  animation: dialog-enter 0.2s ease-out;
}

@keyframes dialog-enter {
  from {
    opacity: 0;
    transform: scale(0.96) translateY(8px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
}

.history-dialog__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-5, 20px) var(--space-6, 24px);
  border-bottom: 1px solid var(--border-subtle, #eef0f4);
}

.history-dialog__title-row {
  display: flex;
  align-items: center;
  gap: var(--space-3, 12px);
}

.history-dialog__title-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border-radius: 12px;
  background: var(--accent-subtle, rgba(94, 106, 210, 0.1));
  color: var(--accent, #5e6ad2);
}

.history-dialog__title {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-primary, #1a1d23);
}

.history-dialog__subtitle {
  margin: 2px 0 0;
  font-size: 11px;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--text-muted, #8b919d);
}

.history-dialog__body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr);
  flex: 1;
  min-height: 0;
  border-top: 1px solid var(--border-subtle, #eef0f4);
}

.history-dialog__col {
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.history-dialog__col--sessions {
  border-right: 1px solid var(--border-subtle, #eef0f4);
}

.history-dialog__col-header {
  padding: var(--space-3, 12px) var(--space-5, 20px);
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.12em;
  color: var(--text-muted, #8b919d);
  border-bottom: 1px solid var(--border-subtle, #eef0f4);
  background: var(--surface-overlay, #f0f1f5);
}

.history-dialog__col-subtitle {
  display: block;
  margin-top: 2px;
  font-size: 11px;
  font-weight: 400;
  text-transform: none;
  letter-spacing: 0;
  color: var(--text-secondary, #5c6370);
}

.history-dialog__col-content {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-3, 12px);
}

.history-dialog__loading,
.history-dialog__empty {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 120px;
  color: var(--text-muted, #8b919d);
  font-size: 14px;
  text-align: center;
  border: 1px dashed var(--border-default, #e2e5eb);
  border-radius: var(--radius-md, 6px);
  padding: var(--space-4, 16px);
}

.history-dialog__list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2, 8px);
}

.history-dialog__session-item {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: var(--space-1, 4px);
  padding: var(--space-3, 12px);
  background: transparent;
  border: 1px solid var(--border-default, #e2e5eb);
  border-radius: var(--radius-md, 6px);
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
}

.history-dialog__session-item:hover {
  background: var(--accent-subtle, rgba(94, 106, 210, 0.06));
  border-color: var(--accent, #5e6ad2);
}

.history-dialog__session-item.active {
  background: var(--accent-subtle, rgba(94, 106, 210, 0.08));
  border-color: var(--accent, #5e6ad2);
}

.history-dialog__session-info {
  display: flex;
  align-items: center;
  gap: var(--space-2, 8px);
}

.history-dialog__session-name {
  flex: 1;
  font-size: 14px;
  font-weight: 590;
  color: var(--text-primary, #1a1d23);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.history-dialog__active-dot {
  color: var(--accent, #5e6ad2);
  font-size: 12px;
  flex-shrink: 0;
}

.history-dialog__session-meta {
  display: flex;
  align-items: center;
  gap: var(--space-3, 12px);
  font-size: 11px;
  color: var(--text-muted, #8b919d);
}

.history-dialog__item-actions {
  position: absolute;
  top: var(--space-2, 8px);
  right: var(--space-2, 8px);
  display: flex;
  gap: 4px;
  opacity: 0;
  transition: opacity 0.15s;
}

.history-dialog__session-item:hover .history-dialog__item-actions {
  opacity: 1;
}

.history-dialog__action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: var(--surface-overlay, #f0f1f5);
  border-radius: var(--radius-sm, 4px);
  color: var(--text-secondary, #5c6370);
  cursor: pointer;
  font-size: 12px;
  transition: background 0.1s, color 0.1s;
}

.history-dialog__action-btn:hover {
  background: var(--border-default, #e2e5eb);
  color: var(--text-primary, #1a1d23);
}

.history-dialog__action-btn--danger:hover {
  background: var(--status-danger-bg, rgba(239, 68, 68, 0.1));
  color: var(--status-danger, #ef4444);
}

.history-dialog__inline-actions {
  display: flex;
  gap: var(--space-2, 8px);
  margin-top: var(--space-2, 8px);
}

.history-dialog__footer {
  padding: var(--space-3, 12px) var(--space-6, 24px);
  text-align: center;
  font-size: 11px;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted, #8b919d);
  background: var(--surface-overlay, #f0f1f5);
  border-top: 1px solid var(--border-subtle, #eef0f4);
}

.history-dialog__rename-overlay {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.3);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10;
}

.history-dialog__rename-box {
  width: min(420px, 90%);
  background: var(--surface-base, #fff);
  border: 1px solid var(--border-default, #e2e5eb);
  border-radius: var(--radius-lg, 8px);
  padding: var(--space-5, 20px);
  display: flex;
  flex-direction: column;
  gap: var(--space-3, 12px);
}

.history-dialog__rename-box h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary, #1a1d23);
}

.history-dialog__rename-box p {
  margin: 0;
  font-size: 13px;
  color: var(--text-muted, #8b919d);
}

.history-dialog__rename-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2, 8px);
}
</style>
