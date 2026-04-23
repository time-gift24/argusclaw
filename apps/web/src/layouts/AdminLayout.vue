<script setup lang="ts">
import { computed } from "vue";
import { RouterLink, RouterView, useRoute } from "vue-router";

import { adminNavItems } from "@/app/nav";

const route = useRoute();
const currentItem = computed(() => {
  return adminNavItems.find((item) => item.to === route.path) ?? adminNavItems[0];
});
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
            <h1>ArgusClaw</h1>
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
          <p class="header-eyebrow">管理中心</p>
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
  border-right: 1px solid var(--border-default);
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
  font-weight: 500;
  transition: background 120ms ease, color 120ms ease;
}

.nav-item:hover {
  background: var(--accent-subtle);
  color: var(--text-primary);
}

.nav-item.active {
  background: var(--accent-subtle);
  color: var(--accent);
  font-weight: 590;
}

.nav-item__label {
  font-size: var(--text-sm);
}

.sidebar-footer {
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
  font-weight: 500;
  color: var(--success);
}

.instance-dot {
  width: 6px;
  height: 6px;
  background: var(--success);
  border-radius: 50%;
}

.route-shell {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
  padding: var(--space-6);
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
    border-bottom: 1px solid var(--border-default);
  }

  .route-shell {
    padding: var(--space-4);
  }
}
</style>
