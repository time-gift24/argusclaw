<template>
  <PageBodyShell title="LLM 提供商" :breadcrumbs="['LLM 提供商']">
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
      <div v-for="p in providers" :key="p.id" class="p-4 bg-surface-container-lowest rounded-xl border border-outline-variant/30">
        <div class="flex justify-between items-center mb-2">
          <h3 class="font-headline font-semibold text-sm">{{ p.name }}</h3>
          <span :class="p.status === 'active' ? 'text-success bg-success-bg' : 'text-text-muted bg-surface-container'" class="text-xs px-2 py-0.5 rounded-full">{{ p.status === 'active' ? '运行中' : '未启用' }}</span>
        </div>
        <p class="text-xs text-text-muted font-mono">{{ p.model }}</p>
      </div>
    </div>
    <p class="text-xs text-text-muted mt-6">@mock - 等待后端 Provider CRUD API</p>
  </PageBodyShell>
</template>

<script setup>
import PageBodyShell from '../components/PageBodyShell.vue'

// @mock
const providers = [
  { id: 1, name: 'OpenAI GPT-4o', model: 'gpt-4o', status: 'active' },
  { id: 2, name: 'Claude Sonnet', model: 'claude-sonnet-4-6', status: 'active' },
  { id: 3, name: 'DeepSeek V3', model: 'deepseek-v3', status: 'inactive' },
]
</script>
