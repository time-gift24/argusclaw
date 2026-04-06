<template>
  <div class="flex h-[calc(100vh-3.5rem-3rem)] -m-6">
    <!-- Session sidebar -->
    <div class="w-56 border-r border-outline-variant/30 bg-surface-container-low flex flex-col shrink-0">
      <div class="p-3 border-b border-outline-variant/30">
        <button
          class="w-full px-3 py-2 text-xs font-semibold bg-primary text-on-primary rounded-lg hover:opacity-90 transition-opacity cursor-pointer"
          @click="handleNewSession"
        >
          + 新建会话
        </button>
      </div>
      <div class="flex-1 overflow-y-auto p-2 space-y-0.5">
        <button
          v-for="session in chatStore.sessions"
          :key="session.id"
          class="w-full text-left px-3 py-2 rounded-lg text-sm transition-colors cursor-pointer truncate"
          :class="session.id === chatStore.currentSessionId
            ? 'bg-primary/10 text-primary font-medium'
            : 'text-on-surface-variant hover:bg-surface-container'"
          @click="handleSelectSession(session)"
        >
          {{ session.name || '未命名会话' }}
        </button>
        <div v-if="!chatStore.sessions.length" class="text-xs text-text-muted text-center py-4">
          暂无会话
        </div>
      </div>
    </div>

    <!-- Thread sidebar (when session selected) -->
    <div v-if="chatStore.currentSessionId" class="w-48 border-r border-outline-variant/30 bg-surface-container-lowest flex flex-col shrink-0">
      <div class="p-3 border-b border-outline-variant/30 text-xs font-semibold text-on-surface-variant">
        对话线程
      </div>
      <div class="flex-1 overflow-y-auto p-2 space-y-0.5">
        <button
          v-for="thread in chatStore.threads"
          :key="thread.id"
          class="w-full text-left px-3 py-2 rounded-lg text-xs transition-colors cursor-pointer truncate"
          :class="thread.id === chatStore.currentThreadId
            ? 'bg-primary/10 text-primary font-medium'
            : 'text-on-surface-variant hover:bg-surface-container'"
          @click="chatStore.selectThread(thread.id)"
        >
          {{ thread.name || thread.id?.slice(0, 8) || '线程' }}
        </button>
      </div>
    </div>

    <!-- Chat area -->
    <div class="flex-1 flex flex-col min-w-0">
      <div v-if="!chatStore.currentThreadId" class="flex-1 flex items-center justify-center text-on-surface-variant">
        <div class="text-center">
          <p class="text-lg font-headline">选择或新建一个会话开始对话</p>
          <p class="text-sm mt-2 text-text-muted">在左侧选择会话和线程</p>
        </div>
      </div>

      <template v-else>
        <!-- Messages area -->
        <div class="flex-1 overflow-y-auto p-4">
          <tr-bubble-list
            :items="bubbleItems"
            :roles="bubbleRoles"
            :auto-scroll="true"
          />
        </div>

        <!-- Input area -->
        <div class="border-t border-outline-variant/30 p-3">
          <tr-sender
            :disabled="chatStore.sending"
            @submit="handleSend"
          />
        </div>
      </template>
    </div>
  </div>
</template>

<script setup>
import { computed, onMounted, onUnmounted } from 'vue'
import { useChatStore } from '../stores/chat'
import { TrBubbleList, TrSender } from '@opentiny/tiny-robot'

const chatStore = useChatStore()

const bubbleRoles = {
  user: { placement: 'end' },
  ai: { placement: 'start' },
  tool: { placement: 'start' },
}

const bubbleItems = computed(() =>
  chatStore.messages.map((msg) => ({
    id: msg.id,
    role: msg.role,
    content: msg.content || '',
    loading: msg.loading,
    aborted: msg.error,
  }))
)

async function handleNewSession() {
  const name = `会话 ${chatStore.sessions.length + 1}`
  await chatStore.newSession(name)
  await chatStore.fetchThreads()
}

function handleSelectSession(session) {
  chatStore.selectSession(session.id)
  chatStore.fetchThreads()
}

async function handleSend({ content }) {
  if (!content?.trim()) return
  await chatStore.send(content.trim())
}

onMounted(() => {
  chatStore.fetchSessions()
  chatStore.fetchAgents()
})

onUnmounted(() => {
  chatStore.disconnectSSE()
})
</script>
