import { createRouter, createWebHistory } from 'vue-router'

const routes = [
  { path: '/', redirect: '/chat' },
  {
    path: '/chat',
    name: 'chat',
    component: () => import('../views/ChatView.vue'),
    meta: { requiresAuth: true, title: '对话' },
  },
  {
    path: '/sessions',
    name: 'sessions',
    component: () => import('../views/SessionsView.vue'),
    meta: { requiresAuth: true, title: '会话' },
  },
  {
    path: '/agents',
    name: 'agents',
    component: () => import('../views/AgentsView.vue'),
    meta: { requiresAuth: true, title: '智能体' },
  },
  {
    path: '/providers',
    name: 'providers',
    component: () => import('../views/ProvidersView.vue'),
    meta: { requiresAuth: true, title: 'LLM 提供商' },
  },
  {
    path: '/mcp',
    name: 'mcp',
    component: () => import('../views/McpConfigsView.vue'),
    meta: { requiresAuth: true, title: 'MCP 配置' },
  },
  {
    path: '/tools',
    name: 'tools',
    component: () => import('../views/ToolsView.vue'),
    meta: { requiresAuth: true, title: '工具库' },
  },
  {
    path: '/settings',
    name: 'settings',
    component: () => import('../views/SettingsView.vue'),
    meta: { requiresAuth: true, title: '设置' },
  },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

export default router
