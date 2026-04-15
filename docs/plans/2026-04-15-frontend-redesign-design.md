# 前端重设计设计方案

## 概述

按照 DESIGN.md 的暖色纸张风格全新设计 UI，同时优化排版、字体和交互逻辑。保留 `@assistant-ui/react` runtime 逻辑，基于其 primitives 完全重写 UI 层。

## 技术路径

- **技术栈**：React 19 + Vite + Tauri 2（不变）
- **聊天 runtime**：保留 `@assistant-ui/react` 的状态管理、消息流转、session 管理
- **UI 层**：基于 `ComposerPrimitive`、`MessagePrimitive`、`ThreadPrimitive` 重写组件
- **样式**：Tailwind CSS + CSS 变量，`globals.css` 实现 DESIGN.md 色彩系统
- **主题**：专注浅色主题，暂不实现深色模式

## 页面结构

```
/ (ChatScreen)
├── 极简顶部栏（logo + 最小化操作）
├── 聊天区（全屏沉浸）
└── 设置浮层（按需展开）

/settings/*
├── agents/
├── providers/
├── tools/
└── mcp/
```

---

## 第一部分：色彩与字体系统

### CSS 变量

| Token | Value | Role |
|-------|-------|------|
| `--color-parchment` | `#f5f4ed` | 主背景（羊皮纸暖白） |
| `--color-ivory` | `#faf9f5` | 卡片/容器背景 |
| `--color-near-black` | `#141413` | 主文字、深色表面 |
| `--color-terracotta` | `#c96442` | 品牌强调、CTA |
| `--color-coral` | `#d97757` | 次要强调 |
| `--color-charcoal` | `#4d4c48` | 按钮文字、次要文字 |
| `--color-olive` | `#5e5d59` | 正文次要文字 |
| `--color-stone` | `#87867f` | 辅助文字、注释 |
| `--color-sand` | `#e8e6dc` | 次要按钮背景 |
| `--color-border-cream` | `#f0eee6` | 浅色边框 |
| `--color-border-warm` | `#e8e6dc` | 强调边框 |
| `--color-dark-surface` | `#30302e` | 深色表面 |

### 字体

| Role | Font | Size | Weight | Line Height |
|------|------|------|--------|-------------|
| Display | Georgia Serif | 48-64px | 500 | 1.10 |
| Heading | Georgia Serif | 28-36px | 500 | 1.20 |
| Body Large | system-ui | 20px | 400 | 1.60 |
| Body | system-ui | 16px | 400 | 1.50 |
| Caption | system-ui | 14px | 400 | 1.43 |
| Code | JetBrains Mono | 15px | 400 | 1.60 |

### 间距韵律

- 基础单位：8px
- 正文行高：1.60（宽松阅读感）
- 标题行高：1.10-1.30（紧凑权威）
- 卡片圆角：8px（标准）、16px（特色）
- 按钮圆角：8px-12px

---

## 第二部分：聊天界面设计

### 全局布局

- 全屏沉浸体验，无顶部导航干扰
- 消息区域占据主要视口
- Composer 固定底部，宽度受限（max-w-2xl）

### 消息呈现

**Assistant 消息：**
- 居中，最大宽度 `72rem`
- Serif 字体，28px 标题
- 正文 Sans-serif，18px，1.60 行高
- 卡片背景 Ivory + Border Cream 边框

**用户消息：**
- 居右，宽度受限
- Sans-serif，低调用色（Olive Gray）
- 圆角气泡背景 Sand

### Composer（输入框）

- 位置：屏幕底部中央
- 宽度：max-w-2xl，水平居中
- 背景：Ivory，ring shadow
- 圆角：24px
- 发送按钮：Terracotta 品牌色，圆形，阴影

### 欢迎页

- Bot 图标 + Serif 大标题"欢迎来到 ArgusWing"
- 副标题 Sans-serif，1.60 行高
- 快速开始建议：柔和卡片网格

### 交互元素

**思考中：**
- 可折叠面板
- 背景：Muted Sand
- 展开/收起动画

**工具调用：**
- 紧凑卡片列表
- 状态图标 + 工具名 + 参数摘要

**消息操作（hover）：**
- 浮动工具栏
- 复制、重新生成、导出

### 顶部控制栏

- 左：新建会话、历史会话
- 中：Agent 选择器、Provider 选择器
- 右：Token 环（上下文使用率）

---

## 第三部分：设置页面设计

### 整体布局

```
┌──────────┬──────────────────────────────────────────┐
│          │                                          │
│  Sidebar │           Content Area                   │
│  240px   │   max-w-7xl centered                    │
│          │                                          │
└──────────┴──────────────────────────────────────────┘
```

### Sidebar 导航

- 宽度：240px，固定定位
- 背景：Ivory
- 右侧分割线：Border Cream
- Logo：ArgusWing 标识

**导航项：**
- Agents、Providers、Tools、MCP
- Hover：Sand 背景
- Active：Terracotta 左边框 + 加粗

**可展开子项（如 Providers）：**
- Model List 子项
- 点击展开/收起

### 内容区

- 面包屑：Stone Gray，简洁
- 页面标题：Serif 28px
- 内容卡片：Ivory + Border Cream，圆角 8px
- 表单：Sand 输入框，Terracotta 聚焦环

### 设置子页面

| Route | Content |
|-------|---------|
| `/settings` | Redirect to `/settings/providers` |
| `/settings/agents` | Agent 列表 + 新建/编辑 |
| `/settings/providers` | Provider 配置 + Model 列表 |
| `/settings/tools` | 内置工具管理 |
| `/settings/mcp` | MCP server 配置 |

---

## 第四部分：实现优先级

### Phase 1：基础建设
1. 更新 `globals.css` CSS 变量
2. 创建基础组件库（按钮、卡片、输入框）
3. 重写布局组件

### Phase 2：聊天界面
4. 重写 Thread/Message/Composer primitives
5. 实现欢迎页
6. 实现消息操作栏

### Phase 3：设置页面
7. 实现 Sidebar 导航
8. 重写各设置子页面

### Phase 4：细节打磨
9. 动画和过渡效果
10. 错误状态和边界情况
