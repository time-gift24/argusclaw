<template>
  <PageBodyShell title="设置" :breadcrumbs="['设置']">
    <div class="max-w-2xl space-y-6">
      <div class="p-5 bg-surface-container-lowest rounded-xl border border-outline-variant/30">
        <h3 class="font-headline font-semibold text-sm mb-3">通用设置</h3>
        <div class="space-y-3">
          <div class="flex justify-between items-center text-sm">
            <span class="text-on-surface-variant">服务器地址</span>
            <span class="font-mono text-xs">localhost:3000</span>
          </div>
        </div>
      </div>
      <p class="text-xs text-text-muted">@mock - 设置页面待实现</p>
    </div>
  </PageBodyShell>
</template>

<script setup>
import PageBodyShell from '../components/PageBodyShell.vue'
</script>
