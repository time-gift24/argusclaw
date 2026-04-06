<template>
  <PageBodyShell title="MCP 配置" :breadcrumbs="['MCP 配置']">
    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
      <div v-for="c in configs" :key="c.id" class="p-4 bg-surface-container-lowest rounded-xl border border-outline-variant/30">
        <h3 class="font-headline font-semibold text-sm mb-1">{{ c.name }}</h3>
        <p class="text-xs text-text-muted">{{ c.transport }} · {{ c.url }}</p>
      </div>
    </div>
    <p class="text-xs text-text-muted mt-6">@mock - 等待后端 MCP Config API</p>
  </PageBodyShell>
</template>

<script setup>
import PageBodyShell from '../components/PageBodyShell.vue'

// @mock
const configs = [
  { id: 1, name: '文件系统', transport: 'stdio', url: '/usr/bin/mcp-fs' },
  { id: 2, name: 'Web 搜索', transport: 'sse', url: 'http://localhost:8080/sse' },
]
</script>
