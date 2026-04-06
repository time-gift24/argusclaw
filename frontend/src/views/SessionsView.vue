<template>
  <PageBodyShell title="会话管理" :breadcrumbs="['对话', '会话']">
    <template #actions>
      <button
        class="px-4 py-2 text-sm font-semibold bg-primary text-on-primary rounded-lg hover:opacity-90 transition-opacity cursor-pointer"
        @click="showCreate = true"
      >
        新建会话
      </button>
    </template>

    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
      <div
        v-for="session in chatStore.sessions"
        :key="session.id"
        class="p-4 bg-surface-container-lowest rounded-xl border border-outline-variant/30 hover:shadow-card transition-shadow cursor-pointer"
        @click="goToChat(session.id)"
      >
        <h3 class="font-headline font-semibold text-sm text-on-surface truncate">{{ session.name || '未命名会话' }}</h3>
        <p class="text-xs text-text-muted mt-1">{{ session.id?.slice(0, 12) }}...</p>
      </div>
    </div>
    <div v-if="!chatStore.sessions.length" class="text-center py-12 text-on-surface-variant">
      暂无会话，点击上方按钮创建
    </div>

    <!-- Create dialog -->
    <div v-if="showCreate" class="fixed inset-0 bg-black/40 z-50 flex items-center justify-center" @click.self="showCreate = false">
      <div class="bg-surface-container-lowest rounded-2xl p-6 w-96 shadow-float">
        <h3 class="font-headline font-bold text-lg mb-4">新建会话</h3>
        <input
          v-model="newName"
          class="w-full px-3 py-2 text-sm bg-surface-container rounded-lg border border-outline-variant/30 focus:border-primary focus:outline-none"
          placeholder="会话名称"
          @keyup.enter="createNew"
        />
        <div class="flex justify-end gap-2 mt-4">
          <button class="px-4 py-2 text-sm text-on-surface-variant hover:bg-surface-container rounded-lg cursor-pointer" @click="showCreate = false">取消</button>
          <button class="px-4 py-2 text-sm bg-primary text-on-primary rounded-lg hover:opacity-90 cursor-pointer" @click="createNew">创建</button>
        </div>
      </div>
    </div>
  </PageBodyShell>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useChatStore } from '../stores/chat'
import PageBodyShell from '../components/PageBodyShell.vue'

const router = useRouter()
const chatStore = useChatStore()
const showCreate = ref(false)
const newName = ref('')

function goToChat(sessionId) {
  router.push({ path: '/chat', query: { session: sessionId } })
}

async function createNew() {
  const name = newName.value.trim() || `会话 ${chatStore.sessions.length + 1}`
  await chatStore.newSession(name)
  showCreate.value = false
  newName.value = ''
}

onMounted(() => {
  chatStore.fetchSessions()
})
</script>
