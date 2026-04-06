<template>
  <div class="min-h-screen bg-surface font-body text-on-surface">
    <!-- Loading state -->
    <div v-if="userStore.loading" class="min-h-screen flex items-center justify-center">
      <div class="text-center text-on-surface-variant">
        <div class="animate-spin w-8 h-8 border-2 border-primary border-t-transparent rounded-full mx-auto mb-4"></div>
        <p class="text-sm">加载中...</p>
      </div>
    </div>

    <!-- Not logged in -->
    <div v-else-if="!userStore.isLoggedIn" class="min-h-screen flex items-center justify-center">
      <div class="text-center">
        <div class="w-16 h-16 rounded-2xl bg-primary flex items-center justify-center text-on-primary mx-auto mb-6">
          <svg class="w-8 h-8" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <rect x="4" y="4" width="16" height="16" rx="2"/>
            <path d="M9 9h.01M15 9h.01M9 15h.01M15 15h.01"/>
          </svg>
        </div>
        <h2 class="text-2xl font-headline font-bold text-on-surface mb-2">Argus</h2>
        <p class="text-on-surface-variant mb-6">智能体管理平台</p>
        <button
          class="px-6 py-2.5 bg-primary text-on-primary rounded-lg font-semibold hover:bg-primary-container transition-colors cursor-pointer"
          @click="userStore.login()"
        >
          登录
        </button>
      </div>
    </div>

    <!-- Main app layout -->
    <div v-else class="flex min-h-screen">
      <!-- Sidebar -->
      <aside
        class="fixed left-0 top-0 h-screen flex flex-col justify-between bg-surface-container-low border-r border-outline-variant/30 z-50 sidebar-transition overflow-hidden"
        :class="isCollapsed ? 'w-16' : 'w-56'"
      >
        <div :class="isCollapsed ? 'space-y-3' : 'space-y-4'">
          <!-- Logo -->
          <div
            class="flex items-center gap-3 px-3 h-14"
            :class="isCollapsed ? 'justify-center' : ''"
          >
            <div class="w-8 h-8 min-w-[32px] rounded-lg bg-primary flex items-center justify-center text-on-primary">
              <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <rect x="4" y="4" width="16" height="16" rx="2"/>
                <path d="M9 9h.01M15 9h.01M9 15h.01M15 15h.01"/>
              </svg>
            </div>
            <div v-if="!isCollapsed" class="overflow-hidden">
              <h3 class="font-headline font-bold text-sm leading-tight">Argus</h3>
            </div>
          </div>

          <!-- Nav items -->
          <nav class="space-y-0.5 px-2">
            <router-link
              v-for="item in sidebarItems"
              :key="item.path"
              :to="item.path"
              class="flex items-center gap-3 text-sm font-medium transition-all rounded-lg overflow-hidden"
              :class="[
                isCollapsed ? 'h-9 w-9 justify-center mx-auto p-0 gap-0' : 'px-3 py-2',
                isActiveRoute(item.path)
                  ? (isCollapsed ? 'bg-primary/10 text-primary' : 'bg-surface-container text-primary shadow-sm')
                  : 'text-on-surface-variant hover:bg-surface-container'
              ]"
            >
              <component :is="item.icon" class="h-4 w-4 min-w-[16px] shrink-0" />
              <span v-if="!isCollapsed" class="whitespace-nowrap">{{ item.name }}</span>
            </router-link>
          </nav>
        </div>

        <!-- Bottom: collapse toggle + user -->
        <div class="border-t border-outline-variant/30 p-2">
          <button
            class="w-full flex items-center gap-2 px-2 py-2 text-on-surface-variant text-sm hover:bg-surface-container rounded-lg transition-colors cursor-pointer"
            :class="isCollapsed ? 'justify-center' : ''"
            @click="isCollapsed = !isCollapsed"
          >
            <svg class="w-4 h-4 shrink-0" :class="isCollapsed ? '' : 'rotate-180'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polyline points="15 18 9 12 15 6" />
            </svg>
            <span v-if="!isCollapsed">收起侧栏</span>
          </button>
        </div>
      </aside>

      <!-- Main content -->
      <div
        class="flex-1 flex flex-col min-h-screen sidebar-transition"
        :class="isCollapsed ? 'ml-16' : 'ml-56'"
      >
        <!-- Top bar -->
        <nav class="sticky top-0 z-40 h-14 flex items-center justify-between px-6 bg-surface-container-low/80 backdrop-blur-md border-b border-outline-variant/30">
          <div class="flex items-center gap-3">
            <span class="text-lg font-bold text-primary font-headline tracking-tight">Argus</span>
          </div>
          <div class="flex items-center gap-3">
            <span class="text-xs text-on-surface-variant">{{ userStore.userName }}</span>
            <button
              class="text-xs text-on-surface-variant hover:text-danger transition-colors cursor-pointer"
              @click="userStore.logout()"
            >
              退出
            </button>
          </div>
        </nav>

        <!-- Page content -->
        <main class="flex-1 p-6">
          <router-view v-slot="{ Component }">
            <Transition name="fade" mode="out-in">
              <component :is="Component" />
            </Transition>
          </router-view>
        </main>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useUserStore } from './stores/user'
import { MessageSquare, List, Bot, Server, Plug, Wrench, Settings } from 'lucide-vue-next'

const route = useRoute()
const userStore = useUserStore()
const isCollapsed = ref(false)

const sidebarItems = [
  { name: '对话', path: '/chat', icon: MessageSquare },
  { name: '会话', path: '/sessions', icon: List },
  { name: '智能体', path: '/agents', icon: Bot },
  { name: 'LLM 提供商', path: '/providers', icon: Server },
  { name: 'MCP 配置', path: '/mcp', icon: Plug },
  { name: '工具库', path: '/tools', icon: Wrench },
  { name: '设置', path: '/settings', icon: Settings },
]

function isActiveRoute(path) {
  if (path === '/') return route.path === '/'
  return route.path.startsWith(path)
}

onMounted(() => {
  userStore.checkAuth()
})
</script>

<style scoped>
.sidebar-transition {
  transition: width 0.3s cubic-bezier(0.4, 0, 0.2, 1),
              margin-left 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}
</style>
