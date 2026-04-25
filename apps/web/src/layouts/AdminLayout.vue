<script setup lang="ts">
import { computed, ref, onMounted } from "vue";
import { RouterLink, RouterView, useRoute } from "vue-router";

import AppBreadcrumb from "@/components/AppBreadcrumb.vue";
import { adminNavItems } from "@/app/nav";

const route = useRoute();
const currentItem = computed(() => {
  return adminNavItems.find((item) => item.to === route.path) ?? adminNavItems[0];
});

const isDark = ref(false);

onMounted(() => {
  const saved = localStorage.getItem("theme");
  if (saved === "dark") {
    isDark.value = true;
    document.documentElement.classList.add("theme-dark");
    document.documentElement.classList.remove("theme-light");
  } else {
    isDark.value = false;
    document.documentElement.classList.add("theme-light");
    document.documentElement.classList.remove("theme-dark");
  }
});

function toggleTheme() {
  isDark.value = !isDark.value;
  if (isDark.value) {
    document.documentElement.classList.add("theme-dark");
    document.documentElement.classList.remove("theme-light");
    localStorage.setItem("theme", "dark");
  } else {
    document.documentElement.classList.add("theme-light");
    document.documentElement.classList.remove("theme-dark");
    localStorage.setItem("theme", "light");
  }
}
</script>

<template>
  <div class="admin-shell">
    <aside class="sidebar">
      <div class="sidebar__inner">
        <div class="brand-block">
          <div class="brand-logo">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
              <circle cx="12" cy="12" r="10" stroke="currentColor" stroke-width="1.5"/>
              <circle cx="12" cy="12" r="4" fill="currentColor"/>
            </svg>
          </div>
          <div class="brand-text">
            <h1>ArgusWing</h1>
            <span class="brand-tag">管理控制台</span>
          </div>
        </div>

        <div class="sidebar-section">
          <p class="sidebar-label">导航</p>
          <nav
            class="nav-list"
            aria-label="管理导航"
          >
            <RouterLink
              v-for="item in adminNavItems"
              :key="item.key"
              :to="item.to"
              class="nav-item"
              :class="{ active: route.path === item.to }"
            >
              <span class="nav-item__label">{{ item.label }}</span>
            </RouterLink>
          </nav>
        </div>

        <div class="sidebar-footer">
          <button class="theme-toggle" @click="toggleTheme" :title="isDark ? '切换到浅色模式' : '切换到深色模式'">
            <!-- Sun icon (shown in dark mode to switch to light) -->
            <svg v-if="isDark" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <circle cx="12" cy="12" r="5"/>
              <line x1="12" y1="1" x2="12" y2="3"/>
              <line x1="12" y1="21" x2="12" y2="23"/>
              <line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/>
              <line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/>
              <line x1="1" y1="12" x2="3" y2="12"/>
              <line x1="21" y1="12" x2="23" y2="12"/>
              <line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/>
              <line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>
            </svg>
            <!-- Moon icon (shown in light mode to switch to dark) -->
            <svg v-else width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>
            </svg>
            <span>{{ isDark ? '浅色模式' : '深色模式' }}</span>
          </button>
          <div class="instance-badge">
            <span class="instance-dot"></span>
            <span>单实例模式</span>
          </div>
        </div>
      </div>
    </aside>

    <main class="route-shell">
      <section class="route-header">
        <div class="header-content">
          <AppBreadcrumb />
          <h2 class="header-title">{{ currentItem.label }}</h2>
          <p class="header-description">
            {{ currentItem.description }}
          </p>
        </div>
      </section>
      <RouterView />
    </main>
  </div>
</template>

<style scoped>
.admin-shell {
  display: grid;
  grid-template-columns: 260px minmax(0, 1fr);
  min-height: 100vh;
  background: var(--app-bg);
}

.sidebar {
  position: sticky;
  top: 0;
  height: 100vh;
  background: var(--surface-base);
  border-right: 1px solid var(--border-subtle);
  display: flex;
  flex-direction: column;
}

.sidebar__inner {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--space-5);
}

.brand-block {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding-bottom: var(--space-5);
  border-bottom: 1px solid var(--border-subtle);
  margin-bottom: var(--space-5);
}

.brand-logo {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent);
  color: white;
  border-radius: var(--radius-md);
}

.brand-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.brand-text h1 {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
  letter-spacing: -0.1px;
}

.brand-tag {
  font-size: var(--text-xs);
  color: var(--text-muted);
}

.sidebar-section {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.sidebar-label {
  margin: 0 0 var(--space-2);
  font-size: var(--text-xs);
  font-weight: 590;
  color: var(--text-placeholder);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.nav-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.nav-item {
  display: flex;
  align-items: center;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-weight: 510;
  transition:
    background var(--transition-base),
    color var(--transition-base),
    transform var(--transition-fast);
  cursor: pointer;
}

.nav-item:hover {
  background: var(--accent-subtle);
  color: var(--text-primary);
}

.nav-item.active {
  background: var(--accent-subtle);
  color: var(--accent);
  font-weight: 590;
  border-left: 3px solid var(--accent);
  padding-left: calc(var(--space-3) - 3px);
}

.nav-item:active {
  transform: scale(0.97);
}

.nav-item__label {
  font-size: var(--text-sm);
}

.sidebar-footer {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding-top: var(--space-4);
  border-top: 1px solid var(--border-subtle);
}

.instance-badge {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--success-bg);
  border: 1px solid var(--success-border);
  border-radius: var(--radius-full);
  font-size: var(--text-xs);
  font-weight: 510;
  color: var(--success);
}

.instance-dot {
  width: 6px;
  height: 6px;
  background: var(--success);
  border-radius: 50%;
}

.theme-toggle {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-2) var(--space-3);
  background: transparent;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-weight: 510;
  cursor: pointer;
  transition: all var(--transition-base);
}

.theme-toggle:hover {
  background: var(--accent-subtle);
  border-color: var(--accent);
  color: var(--accent);
}

.route-shell {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
  padding: var(--space-6);
  width: 100%;
}

.route-shell > :not(.route-header) {
  max-width: 1200px;
}

.route-header {
  padding: var(--space-5) var(--space-6);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.header-content {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.header-eyebrow {
  margin: 0;
  font-size: var(--text-xs);
  font-weight: 590;
  color: var(--accent);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.header-title {
  margin: 0;
  font-size: var(--text-2xl);
  font-weight: 590;
  color: var(--text-primary);
  letter-spacing: -0.24px;
}

.header-description {
  margin: var(--space-1) 0 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
  line-height: 1.5;
}

@media (max-width: 960px) {
  .admin-shell {
    grid-template-columns: 1fr;
  }

  .sidebar {
    position: static;
    height: auto;
    border-right: 0;
    border-bottom: 1px solid var(--border-subtle);
  }

  .route-shell {
    padding: var(--space-4);
  }
}
</style>
