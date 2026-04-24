# Admin Console 精致科技感美化实施计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Admin Console 管理后台从浅色主题升级为 Linear 风格深色主题，实现精致科技感视觉体验。

**Architecture:** 以 CSS 变量（tokens.css）为核心，构建 Dark Theme 色彩系统。全局过渡动画变量统一动效时序。具体页面组件通过 CSS 变量实现深色背景层级、微交互反馈、滚动条样式等细节。

**Tech Stack:** CSS Variables, TinyVue, Vue 3 Composition API

---

## 文件结构

| 文件 | 职责 |
|------|------|
| `apps/web/src/styles/tokens.css` | 全局 CSS 变量（色彩、间距、圆角、阴影、过渡、字体） |
| `apps/web/src/layouts/AdminLayout.vue` | 侧边栏 + 主内容区样式（深色背景、微交互） |
| `apps/web/src/features/providers/ProvidersPage.vue` | 提供方列表页（卡片、徽章） |
| `apps/web/src/features/providers/ProviderForm.vue` | 提供方表单（输入框、按钮、选择器） |
| `apps/web/src/features/templates/TemplatesPage.vue` | 模板页（卡片、徽章） |
| `apps/web/src/features/mcp/McpPage.vue` | MCP 服务页（卡片、徽章） |
| `apps/web/src/features/health/HealthPage.vue` | 健康状态页（状态指示、指标卡） |
| `apps/web/src/features/bootstrap/BootstrapPage.vue` | 引导页（指标卡） |

---

## Chunk 1: 全局 tokens.css 深色主题变量

**Files:**
- Modify: `apps/web/src/styles/tokens.css`

---

- [ ] **Step 1: 备份并重写 tokens.css 为深色主题**

替换整个文件内容为：

```css
:root {
  /* === 背景层级（从深到浅）=== */
  --app-bg: #08090a;
  --surface-base: #0f1011;
  --surface-raised: #191a1b;
  --surface-overlay: #28282c;

  /* === 边框 === */
  --border-default: rgba(255, 255, 255, 0.08);
  --border-subtle: rgba(255, 255, 255, 0.05);
  --border-strong: rgba(255, 255, 255, 0.12);

  /* === 文字层级 === */
  --text-primary: #f7f8f8;
  --text-secondary: #d0d6e0;
  --text-muted: #8a8f98;
  --text-placeholder: #62666d;

  /* === 品牌强调色 === */
  --accent: #5e6ad2;
  --accent-hover: #7170ff;
  --accent-subtle: rgba(94, 106, 210, 0.12);

  /* === 状态色 === */
  --success: #10b981;
  --success-bg: rgba(16, 185, 129, 0.12);
  --success-border: rgba(16, 185, 129, 0.25);
  --danger: #ef4444;
  --danger-bg: rgba(239, 68, 68, 0.12);
  --danger-border: rgba(239, 68, 68, 0.25);
  --warning: #f59e0b;
  --warning-bg: rgba(245, 158, 11, 0.12);
  --info: #3b82f6;
  --info-bg: rgba(59, 130, 246, 0.12);

  /* === 间距 === */
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 20px;
  --space-6: 24px;
  --space-8: 32px;
  --space-10: 40px;
  --space-12: 48px;

  /* === 圆角 === */
  --radius-sm: 4px;
  --radius-md: 6px;
  --radius-lg: 8px;
  --radius-xl: 12px;
  --radius-full: 9999px;

  /* === 阴影 === */
  --shadow-xs: 0 1px 2px rgba(0, 0, 0, 0.3);
  --shadow-sm: 0 2px 4px rgba(0, 0, 0, 0.3);

  /* === 过渡 === */
  --transition-fast: 80ms ease-out;
  --transition-base: 120ms ease-out;
  --transition-slow: 200ms ease-out;

  /* === 字体 === */
  --font-sans: "Inter Variable", "Noto Sans SC", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif;
  --font-mono: "Berkeley Mono", "SFMono-Regular", ui-monospace, monospace;

  /* === 字号 === */
  --text-xs: 0.75rem;
  --text-sm: 0.8125rem;
  --text-base: 0.875rem;
  --text-lg: 1rem;
  --text-xl: 1.125rem;
  --text-2xl: 1.25rem;
  --text-3xl: 1.5rem;
}

* {
  box-sizing: border-box;
}

html,
body,
#app {
  min-height: 100%;
}

body {
  margin: 0;
  background: var(--app-bg);
  color: var(--text-primary);
  font-family: var(--font-sans);
  font-feature-settings: "cv01", "ss03";
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

html {
  scroll-behavior: smooth;
}

a {
  color: inherit;
  text-decoration: none;
}

button,
input,
textarea {
  font: inherit;
}

/* === 滚动条样式 === */
::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}
::-webkit-scrollbar-track {
  background: transparent;
}
::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.1);
  border-radius: 3px;
}
::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.18);
}

/* === 文字选中 === */
::selection {
  background: rgba(94, 106, 210, 0.3);
  color: #f7f8f8;
}

/* === 全局卡片样式 === */
.shell-card {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.section-heading {
  margin: 0;
  font-size: var(--text-2xl);
  font-weight: 590;
  letter-spacing: -0.24px;
  color: var(--text-primary);
}

.section-copy {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--text-sm);
  line-height: 1.6;
}

.eyebrow {
  margin: 0 0 var(--space-2);
  color: var(--accent);
  font-size: var(--text-xs);
  font-weight: 590;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}
```

- [ ] **Step 2: 验证文件无语法错误**

检查 CSS 变量完整性（无闭合错误）

---

## Chunk 2: AdminLayout.vue 深色样式

**Files:**
- Modify: `apps/web/src/layouts/AdminLayout.vue`

---

- [ ] **Step 1: 更新 AdminLayout.vue 样式部分**

将 `<style scoped>` 部分替换为：

```css
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
}

.nav-item:active {
  transform: scale(0.97);
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
  font-weight: 510;
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
    border-bottom: 1px solid var(--border-subtle);
  }

  .route-shell {
    padding: var(--space-4);
  }
}
</style>
```

---

## Chunk 3: ProvidersPage.vue + ProviderForm.vue 深色样式

**Files:**
- Modify: `apps/web/src/features/providers/ProvidersPage.vue`
- Modify: `apps/web/src/features/providers/ProviderForm.vue`

---

- [ ] **Step 1: 更新 ProvidersPage.vue 样式**

将 `<style scoped>` 部分替换为：

```css
<style scoped>
.page-grid {
  display: grid;
  gap: var(--space-5);
  align-items: start;
  width: 100%;
}

.form-panel,
.list-panel {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  transition: border-color var(--transition-base);
}

.form-panel {
  width: 100%;
  padding: var(--space-5);
  display: grid;
  gap: var(--space-5);
}

.list-panel {
  padding: var(--space-5);
  display: grid;
  gap: var(--space-4);
}

.panel-header {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.panel-header-left {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.panel-title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
}

.panel-description {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.provider-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.provider-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--space-4);
  padding: var(--space-4);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  transition:
    border-color var(--transition-base),
    transform var(--transition-fast);
}

.provider-card:hover {
  border-color: var(--border-default);
}

.provider-card:active {
  transform: scale(0.99);
}

.provider-info {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  flex: 1 1 320px;
  min-width: 0;
}

.provider-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.provider-name {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
}

.provider-url {
  font-size: var(--text-xs);
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.provider-model {
  font-size: var(--text-xs);
  color: var(--text-placeholder);
}

.empty-state {
  padding: var(--space-8) var(--space-4);
  text-align: center;
  color: var(--text-muted);
  font-size: var(--text-sm);
}

.error-message {
  margin: 0;
  padding: var(--space-3);
  background: var(--danger-bg);
  border: 1px solid var(--danger-border);
  border-radius: var(--radius-md);
  color: var(--danger);
  font-size: var(--text-sm);
}

@media (max-width: 1024px) {
  .page-grid {
    grid-template-columns: 1fr;
  }
}
</style>
```

- [ ] **Step 2: 更新 ProviderForm.vue 样式**

将 `<style scoped>` 部分替换为：

```css
<style scoped>
.provider-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}

.provider-form__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.provider-form__grid :deep(.tiny-form-item) {
  margin-bottom: 0;
}

.provider-form__switch {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.switch-hint {
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.provider-form__actions {
  display: flex;
  gap: var(--space-3);
  flex-wrap: wrap;
}

.full-width {
  grid-column: 1 / -1;
}

@media (max-width: 960px) {
  .provider-form__grid {
    grid-template-columns: 1fr;
  }
}
</style>
```

---

## Chunk 5: TemplatesPage.vue 深色样式

**Files:**
- Modify: `apps/web/src/features/templates/TemplatesPage.vue`

---

- [ ] **Step 1: 更新 TemplatesPage.vue 样式**

将 `<style scoped>` 部分替换为：

```css
<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.page-header-left {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.page-title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
}

.template-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.template-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-5);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  transition:
    border-color var(--transition-base),
    transform var(--transition-fast);
}

.template-card:hover {
  border-color: var(--border-strong);
}

.template-card:active {
  transform: scale(0.99);
}

.template-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.template-name {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
}

.template-version {
  font-size: var(--text-xs);
  font-weight: 510;
  color: var(--text-muted);
  padding: var(--space-1) var(--space-2);
  background: var(--surface-raised);
  border-radius: var(--radius-sm);
}

.template-description {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
  line-height: 1.5;
}

.loading-state,
.empty-state {
  padding: var(--space-10) var(--space-4);
  text-align: center;
  color: var(--text-muted);
  font-size: var(--text-sm);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
}

@media (max-width: 960px) {
  .template-grid {
    grid-template-columns: 1fr;
  }
}
</style>
```

---

## Chunk 6: McpPage.vue 深色样式

**Files:**
- Modify: `apps/web/src/features/mcp/McpPage.vue`

---

- [ ] **Step 1: 更新 McpPage.vue 样式**

将 `<style scoped>` 部分替换为：

```css
<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.page-header-left {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.page-title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
}

.server-list {
  display: grid;
  gap: var(--space-3);
}

.server-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4) var(--space-5);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  transition:
    border-color var(--transition-base),
    transform var(--transition-fast);
}

.server-card:hover {
  border-color: var(--border-strong);
}

.server-card:active {
  transform: scale(0.99);
}

.server-info {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.server-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.server-name {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
}

.server-transport,
.server-tools {
  font-size: var(--text-xs);
  color: var(--text-muted);
}

.loading-state,
.empty-state {
  padding: var(--space-10) var(--space-4);
  text-align: center;
  color: var(--text-muted);
  font-size: var(--text-sm);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
}
</style>
```

---

## Chunk 7: HealthPage.vue + BootstrapPage.vue 深色样式

**Files:**
- Modify: `apps/web/src/features/health/HealthPage.vue`
- Modify: `apps/web/src/features/bootstrap/BootstrapPage.vue`

---

- [ ] **Step 1: 更新 HealthPage.vue 样式**

将 `<style scoped>` 部分替换为：

```css
<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.status-banner {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-5) var(--space-6);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.status-info {
  display: flex;
  align-items: center;
  gap: var(--space-4);
}

.status-indicator {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.status-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--text-placeholder);
  transition: background var(--transition-base);
}

.status-dot.healthy {
  background: var(--success);
}

.status-dot.unhealthy {
  background: var(--danger);
}

.status-label {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
}

.instance-name {
  font-size: var(--text-sm);
  color: var(--text-muted);
  padding-left: var(--space-4);
  border-left: 1px solid var(--border-subtle);
}

.metrics-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--space-4);
}

.metric-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-5);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.metric-label {
  font-size: var(--text-xs);
  font-weight: 510;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.metric-value {
  font-size: var(--text-xl);
  font-weight: 590;
  color: var(--text-primary);
  letter-spacing: -0.16px;
}

@media (max-width: 960px) {
  .metrics-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
```

- [ ] **Step 2: 更新 BootstrapPage.vue 样式**

将 `<style scoped>` 部分替换为：

```css
<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.metrics-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--space-4);
}

.metric-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-5);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.metric-label {
  font-size: var(--text-xs);
  font-weight: 510;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.metric-value {
  font-size: var(--text-xl);
  font-weight: 590;
  color: var(--text-primary);
  letter-spacing: -0.16px;
}

@media (max-width: 960px) {
  .metrics-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
```

---

## Chunk 8: 验证与检查

**Files:**
- All above files

---

- [ ] **Step 1: 运行构建验证**

```bash
cd /Users/wanyaozhong/Projects/argusclaw/.worktrees/desktop-server-web-design && pnpm --filter web build 2>&1 | head -50
```

预期：无编译错误

- [ ] **Step 2: 检查 TypeScript 类型**

```bash
cd /Users/wanyaozhong/Projects/argusclaw/.worktrees/desktop-server-web-design && pnpm --filter web type-check 2>&1 | head -30
```

预期：无类型错误

- [ ] **Step 3: 确认视觉验收标准覆盖**

对照 `docs/superpowers/specs/2026-04-23-admin-console-visual-polish.md` 第 10 节验收标准检查实现

---

## 实施顺序

1. **Chunk 1** → tokens.css（全局变量，是所有其他样式的基础）
2. **Chunk 2** → AdminLayout.vue（框架布局）
3. **Chunk 3** → ProvidersPage.vue + ProviderForm.vue（复杂交互页）
4. **Chunk 5** → TemplatesPage.vue
5. **Chunk 6** → McpPage.vue
6. **Chunk 7** → HealthPage.vue + BootstrapPage.vue
7. **Chunk 8** → 验证构建
