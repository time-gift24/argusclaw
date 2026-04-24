# Admin Console 精致科技感美化设计方案

## 1. 概述与目标

将 Admin Console 管理后台从当前的浅色主题升级为 Linear 风格的深色主题，实现精致科技感的视觉体验。重点：深色背景排版、克制的强调色、微妙而精确的微交互动效。

## 2. 色彩系统

### 深色背景层级

| Token | 值 | 用途 |
|-------|----|------|
| `--app-bg` | `#08090a` | 页面背景（最深） |
| `--surface-base` | `#0f1011` | 侧边栏、面板背景 |
| `--surface-raised` | `#191a1b` | 卡片、浮起元素 |
| `--surface-overlay` | `#28282c` | 下拉、弹窗背景 |

### 文字层级

| Token | 值 | 用途 |
|-------|----|------|
| `--text-primary` | `#f7f8f8` | 主要文字、标题 |
| `--text-secondary` | `#d0d6e0` | 正文 |
| `--text-muted` | `#8a8f98` | 次要文字、元数据 |
| `--text-placeholder` | `#62666d` | 占位符、禁用态 |

### 强调色

| Token | 值 | 用途 |
|-------|----|------|
| `--accent` | `#5e6ad2` | 品牌色、CTA |
| `--accent-hover` | `#7170ff` | 悬停态 |
| `--accent-subtle` | `rgba(94,106,210,0.12)` | 悬停背景 |

### 状态色

| Token | 值 | 用途 |
|-------|----|------|
| `--success` | `#10b981` | 成功/在线状态 |
| `--success-bg` | `rgba(16,185,129,0.12)` | 成功背景 |
| `--danger` | `#ef4444` | 危险/错误 |
| `--danger-bg` | `rgba(239,68,68,0.12)` | 危险背景 |

### 边框

| Token | 值 | 用途 |
|-------|----|------|
| `--border-default` | `rgba(255,255,255,0.08)` | 默认边框 |
| `--border-subtle` | `rgba(255,255,255,0.05)` | 细分隔线 |
| `--border-strong` | `rgba(255,255,255,0.12)` | 强调边框 |

## 3. 排版系统

### 字体栈

```css
--font-sans: "Inter Variable", "Noto Sans SC", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif;
--font-mono: "Berkeley Mono", "SFMono-Regular", ui-monospace, monospace;
```

### OpenType 特性

全局启用：`font-feature-settings: "cv01", "ss03"` — 提供更几何化的 Inter 字形

### 字号层级

| Token | 值 | 用途 |
|-------|----|------|
| `--text-xs` | 0.75rem | 标签、上标 |
| `--text-sm` | 0.8125rem | 正文小号 |
| `--text-base` | 0.875rem | 正文 |
| `--text-lg` | 1rem | 大号正文 |
| `--text-xl` | 1.125rem | 副标题 |
| `--text-2xl` | 1.25rem | 标题 |

### 字重使用规范

- 400 (Regular)：阅读文本
- 510 (Medium)：导航、标签、UI 元素
- 590 (Semibold)：标题、强调

## 4. 间距系统

基于 8px 网格：

| Token | 值 |
|-------|----|
| `--space-1` | 4px |
| `--space-2` | 8px |
| `--space-3` | 12px |
| `--space-4` | 16px |
| `--space-5` | 20px |
| `--space-6` | 24px |
| `--space-8` | 32px |
| `--space-10` | 40px |
| `--space-12` | 48px |

## 5. 圆角系统

| Token | 值 | 用途 |
|-------|----|------|
| `--radius-sm` | 4px | 小按钮、徽章 |
| `--radius-md` | 6px | 输入框、中按钮 |
| `--radius-lg` | 8px | 卡片 |
| `--radius-xl` | 12px | 大面板 |
| `--radius-full` | 9999px | 药片形标签 |

## 6. 阴影系统

暗色主题下阴影不可见，使用背景层级代替：

| Token | 值 | 用途 |
|-------|----|------|
| `--shadow-xs` | `0 1px 2px rgba(0,0,0,0.3)` | 卡片微阴影 |
| `--shadow-sm` | `0 2px 4px rgba(0,0,0,0.3)` | 浮起元素 |

## 7. 微交互动效

### 全局过渡配置

```css
--transition-fast: 80ms ease-out;
--transition-base: 120ms ease-out;
--transition-slow: 200ms ease-out;
```

### 按钮交互

| 状态 | 效果 |
|------|------|
| Hover | `transform: scale(1.01)` + 边框变亮，120ms ease-out |
| Active | `transform: scale(0.97)` 下沉感，80ms ease-out |
| Focus | `border-color: var(--accent)` + `box-shadow: 0 0 0 3px rgba(94,106,210,0.25)` |

### 卡片交互

| 状态 | 效果 |
|------|------|
| Hover | 边框变亮 `rgba(255,255,255,0.05)` → `rgba(255,255,255,0.12)`，120ms ease-out |

### 导航项

| 状态 | 效果 |
|------|------|
| Hover | 背景淡入 `--accent-subtle`，120ms ease-out |
| Active | 背景 `--accent-subtle` + 文字 accent 色 |

### 输入框

| 状态 | 效果 |
|------|------|
| Focus | 边框变 accent + 微光晕 |

## 8. 全局细节

### 滚动条样式

```css
::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}
::-webkit-scrollbar-track {
  background: transparent;
}
::-webkit-scrollbar-thumb {
  background: rgba(255,255,255,0.1);
  border-radius: 3px;
}
::-webkit-scrollbar-thumb:hover {
  background: rgba(255,255,255,0.15);
}
```

### 文字选中

```css
::selection {
  background: rgba(94,106,210,0.3);
  color: #f7f8f8;
}
```

### 平滑滚动

```css
html {
  scroll-behavior: smooth;
}
```

## 9. 实施范围

### 涉及文件

1. `apps/web/src/styles/tokens.css` — 全局 CSS 变量重写
2. `apps/web/src/layouts/AdminLayout.vue` — 侧边栏 + 头部深色样式
3. `apps/web/src/features/providers/ProvidersPage.vue` — 卡片样式更新
4. `apps/web/src/features/providers/ProviderForm.vue` — 表单样式更新
5. `apps/web/src/features/templates/TemplatesPage.vue` — 模板卡片
6. `apps/web/src/features/mcp/McpPage.vue` — MCP 页面
7. `apps/web/src/features/health/HealthPage.vue` — 健康状态页
8. `apps/web/src/features/bootstrap/BootstrapPage.vue` — 引导页

### 不涉及（明确排除）

以下内容不在本次美化范围内，属于独立的维护工作：
- `lib/opentiny.ts` — TinyVue 组件导入（保持不变）
- 路由和业务逻辑 — 功能层面不变，仅样式调整
- API 调用 — 功能层面不变
- 图标样式 — 图像资源不在本次范围内

## 10. 验收标准

> **说明：** 以下均为视觉层面的检查，功能回归测试不在本次范围内。

- [ ] 页面背景为 `#08090a`，侧边栏为 `#0f1011`
- [ ] 所有文字在暗色背景上清晰可读
- [ ] 卡片有细微的边框和背景层级
- [ ] 按钮 hover 有微妙的 scale 反馈（120ms ease-out）
- [ ] 导航项 hover 有背景过渡（120ms ease-out）
- [ ] 输入框 focus 有 accent 色边框 + 光晕
- [ ] 滚动条在暗色主题下协调
- [ ] 文字选中显示 accent 色
- [ ] 中文字体正确显示 Noto Sans SC
- [ ] 本次为纯替换型修改（浅色主题移除，无主题切换开关）
