# ArgusWing Desktop

> **Tauri 2.0 桌面应用** - React 19 + TypeScript + Tailwind CSS v4

## 技术栈

| 类别 | 技术 |
|------|------|
| 桌面框架 | Tauri 2.0 |
| UI 框架 | React 19 |
| 语言 | TypeScript |
| 构建工具 | Next.js (仅构建，无 SSR) |
| 样式 | Tailwind CSS v4 |
| UI 组件 | shadcn/ui (CVA + clsx + tailwind-merge) |
| 图标 | @hugeicons/react |
| 聊天 UI | assistant-ui |
| Markdown | react-markdown + Shiki + KaTeX + Mermaid |

## 重要说明

- **无 SSR**: Tauri 桌面应用，所有组件客户端渲染，无需处理水合问题
- **中文优先**: 用户可见文案默认中文，除非已有英文信息架构约束

---

## 导航设计规范

### 布局结构

```
┌─────────────────────────────────────────────────────────┐
│  Header (sticky top-0)                                  │
│  ┌───────────────────────────────────────────────────┐  │
│  │ Logo    Navigation    Theme  Settings  Notif  Avatar│  │
│  │ mx-auto max-w-7xl px-6 py-4                        │  │
│  └───────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────┤
│  Breadcrumb (条件显示)                                  │
│  ┌───────────────────────────────────────────────────┐  │
│  │ ← 设置 / 智能体 / 编辑                              │  │
│  │ mx-auto max-w-7xl px-6 py-2                        │  │
│  └───────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────┤
│  Main Content                                           │
│  ┌───────────────────────────────────────────────────┐  │
│  │ mx-auto max-w-7xl px-6 py-4                        │  │
│  │ {children}                                         │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### 导航组件 (navbar-component-06)

**Header 区域**:
- 高度: `py-4`
- 最大宽度: `max-w-7xl`
- 水平边距: `px-6`
- 背景: `bg-background`
- 定位: `sticky top-0 z-50`

**Breadcrumb 区域**:
- 高度: `py-2`
- 边框: `border-b`
- 返回按钮: 仅在 3 层及以上面包屑时显示
- 面包屑分隔符: `ChevronRight` 图标

### 面包屑路由映射

| 路径 | 面包屑 |
|------|--------|
| `/settings` | 设置 |
| `/settings/providers` | 设置 / LLM 提供者 |
| `/settings/providers/new` | 设置 / LLM 提供者 / 新建 |
| `/settings/providers/[id]` | 设置 / LLM 提供者 / [名称] |
| `/settings/agents` | 设置 / 智能体 |
| `/settings/agents/new` | 设置 / 智能体 / 新建 |
| `/settings/agents/[id]` | 设置 / 智能体 / [名称] |

---

## 字体设计规范

### 字体大小

| 用途 | 大小 | Tailwind |
|------|------|----------|
| 输入框文字 | 14px | `text-sm` |
| 导航文字 | 14px | `text-sm` |
| 面包屑文字 | 14px | `text-sm` |
| 卡片标题 | 14px | `text-sm font-semibold` |
| 卡片描述 | 12px | `text-xs` |
| 页面标题 | 14px | `text-sm font-semibold` |
| 按钮 | 14px | `text-sm` |
| 图标 (导航/按钮) | 16px | `h-4 w-4` |

### 组件字体配置

**Input 组件** (`components/ui/input.tsx`):
```tsx
className="text-sm" // 固定 14px，无响应式覆盖
```

**Label 组件**:
```tsx
className="text-sm" // 14px
```

**Button 组件**:
```tsx
// size="sm": text-xs (12px)
// size="default": text-sm (14px)
```

### 高频操作区约束

- 导航、认证弹窗、下拉菜单等区域，可见文字控制在 **14px - 18px**
- 登录相关图标控制在 **16px** 左右
- 保持紧凑桌面密度，不要为强调少量操作而放大元素

---

## 内容区布局规范

### Settings Layout

```tsx
// app/settings/layout.tsx
<div className="mx-auto w-full max-w-7xl px-6 py-4">
  {children}
</div>
```

### 列表页结构

```tsx
<div className="w-full space-y-4">
  {/* 页面标题 */}
  <div className="flex items-center justify-between">
    <div>
      <h1 className="text-sm font-semibold">标题</h1>
      <p className="text-muted-foreground text-xs">描述</p>
    </div>
    <Button size="sm">操作</Button>
  </div>

  {/* 内容 */}
  <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
    {items}
  </div>
</div>
```

### 编辑页结构

```tsx
<div className="w-full space-y-4">
  {/* 页面标题 (返回按钮在 navbar 中) */}
  <div className="flex items-center justify-between">
    <h1 className="text-sm font-semibold">标题</h1>
    <Button size="sm">保存</Button>
  </div>

  {/* 两栏布局 */}
  <div className="grid grid-cols-2 gap-6">
    <div className="space-y-4">{/* 左侧表单 */}</div>
    <div className="space-y-4">{/* 右侧内容 */}</div>
  </div>
</div>
```

---

## 开发命令

```bash
pnpm dev          # Next.js 开发模式
pnpm tauri dev    # Tauri 开发模式
pnpm build        # 生产构建
pnpm tauri build  # Tauri 生产构建
```
