<template>
  <PageBodyShell title="工具库" :breadcrumbs="['工具库']">
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
      <div v-for="t in tools" :key="t.name" class="p-4 bg-surface-container-lowest rounded-xl border border-outline-variant/30">
        <h3 class="font-headline font-semibold text-sm mb-1">{{ t.name }}</h3>
        <p class="text-xs text-text-secondary">{{ t.description }}</p>
        <span class="text-xs px-2 py-0.5 rounded-full bg-surface-container text-on-surface-variant mt-2 inline-block">{{ t.type }}</span>
      </div>
    </div>
    <p class="text-xs text-text-muted mt-6">@mock - 等待后端 Tools API</p>
  </PageBodyShell>
</template>

<script setup>
import PageBodyShell from '../components/PageBodyShell.vue'

// @mock
const tools = [
  { name: 'Shell', description: '执行 Shell 命令', type: 'builtin' },
  { name: 'Read', description: '读取文件内容', type: 'builtin' },
  { name: 'Grep', description: '搜索文件内容', type: 'builtin' },
  { name: 'Glob', description: '文件模式匹配', type: 'builtin' },
  { name: 'Http', description: 'HTTP 请求', type: 'builtin' },
  { name: 'WriteFile', description: '写入文件', type: 'builtin' },
  { name: 'ListDir', description: '列出目录', type: 'builtin' },
  { name: 'ApplyPatch', description: '应用补丁', type: 'builtin' },
  { name: 'Chrome', description: '浏览器交互', type: 'builtin' },
]
</script>
