<template>
  <PageBodyShell title="智能体" :breadcrumbs="['智能体']">
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
      <div
        v-for="agent in chatStore.agents"
        :key="agent.id"
        class="p-5 bg-surface-container-lowest rounded-xl border border-outline-variant/30 shadow-card"
      >
        <div class="flex items-center gap-3 mb-3">
          <div class="w-10 h-10 rounded-lg bg-primary/10 flex items-center justify-center text-primary">
            <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42"/></svg>
          </div>
          <div>
            <h3 class="font-headline font-semibold text-sm">{{ agent.name || agent.id }}</h3>
            <span class="text-xs px-2 py-0.5 rounded-full bg-success-bg text-success">已启用</span>
          </div>
        </div>
        <p class="text-xs text-text-secondary line-clamp-2">{{ agent.description || '暂无描述' }}</p>
      </div>
    </div>
    <div v-if="!chatStore.agents.length" class="text-center py-12 text-on-surface-variant">
      暂无启用的智能体
    </div>
  </PageBodyShell>
</template>

<script setup>
import { onMounted } from 'vue'
import { useChatStore } from '../stores/chat'
import PageBodyShell from '../components/PageBodyShell.vue'

const chatStore = useChatStore()

onMounted(() => {
  chatStore.fetchAgents()
})
</script>
