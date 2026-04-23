# ArgusWing 管理控制台设计系统

**日期：** 2026-04-23
**版本：** Phase 4E

## 1. 设计原则

ArgusWing 管理控制台是面向运维人员的中文管理界面，采用浅色模式为主基调，支持浅色/深色主题切换。

**核心原则：**
- **清晰高效**：管理台以信息密度和操作效率为核心，避免过度装饰
- **中文优先**：全中文界面，术语统一，标签简洁
- **浅色为本**：默认浅色模式，深色模式为辅助选项
- **OpenTiny 为基**：优先使用 OpenTiny Vue 组件和 token 覆盖，不重造基础组件
- **传统管理台布局**：左侧导航 + 右侧内容区，信息架构稳定

## 2. 主题系统

### 2.1 主题切换

支持浅色（默认）和深色两种主题，通过侧边栏顶部的切换按钮切换选择器状态，主题偏好持久化存储在 localStorage。

根元素 `class` 控制主题：
- `.theme-light` - 浅色模式（默认）
- `.theme-dark` - 深色模式

### 2.2 浅色模式色板

**背景层级：**
- 页面背景（`--app-bg`）：`#f7f8fa`
- 侧边栏背景（`--surface-base`）：`#ffffff`
- 卡片/抬起表面（`--surface-raised`）：`#ffffff`
- 悬浮层（`--surface-overlay`）：`#f0f1f5`
- 输入框背景（`--input-bg`）：`#ffffff`

**边框：**
- 默认边框（`--border-default`）：`#e2e5eb`
- 微弱边框（`--border-subtle`）：`#eef0f4`
- 强调边框（`--border-strong`）：`#c8cdd8`

**文字：**
- 主文字（`--text-primary`）：`#1a1d23`
- 次要文字（`--text-secondary`）：`#5c6370`
- 弱化文字（`--text-muted`）：`#8b919d`
- 占位文字（`--text-placeholder`）：`#a8aeb8`

**品牌色：**
- 主品牌色（`--accent`）：`#5e6ad2`（与 Linear 保持一致的品牌靛蓝）
- 悬浮状态（`--accent-hover`）：`#7170ff`
- 柔和品牌色（`--accent-subtle`）：`rgba(94, 106, 210, 0.1)`

**状态色：**
- 成功（`--status-success`）：`#10b981`
- 成功背景（`--status-success-bg`）：`rgba(16, 185, 129, 0.1)`
- 危险（`--status-danger`）：`#ef4444`
- 危险背景（`--status-danger-bg`）：`rgba(239, 68, 68, 0.1)`
- 警告（`--status-warning`）：`#f59e0b`
- 警告背景（`--status-warning-bg`）：`rgba(245, 158, 11, 0.1)`
- 信息（`--status-info`）：`#3b82f6`
- 信息背景（`--status-info-bg`）：`rgba(59, 130, 246, 0.1)`

**阴影：**
- 小阴影（`--shadow-sm`）：`0 1px 2px rgba(0, 0, 0, 0.05)`
- 中阴影（`--shadow-md`）：`0 4px 6px -1px rgba(0, 0, 0, 0.07)`
- 大阴影（`--shadow-lg`）：`0 10px 15px -3px rgba(0, 0, 0, 0.08)`

### 2.3 深色模式色板

**背景层级：**
- 页面背景（`--app-bg`）：`#08090a`
- 侧边栏背景（`--surface-base`）：`#0f1011`
- 卡片/抬起表面（`--surface-raised`）：`#191a1b`
- 悬浮层（`--surface-overlay`）：`#28282c`
- 输入框背景（`--input-bg`）：`rgba(255, 255, 255, 0.03)`

**边框：**
- 默认边框（`--border-default`）：`rgba(255, 255, 255, 0.08)`
- 微弱边框（`--border-subtle`）：`rgba(255, 255, 255, 0.05)`
- 强调边框（`--border-strong`）：`rgba(255, 255, 255, 0.12)`

**文字：**
- 主文字（`--text-primary`）：`#f7f8f8`
- 次要文字（`--text-secondary`）：`#d0d6e0`
- 弱化文字（`--text-muted`）：`#8a8f98`
- 占位文字（`--text-placeholder`）：`#62666d`

**品牌色、状态色：** 与浅色模式相同。

**阴影（深色模式使用边框而非阴影表达层次）：**
- 小阴影（`--shadow-sm`）：无或极淡
- 中阴影（`--shadow-md`）：无
- 大阴影（`--shadow-lg`）：无

## 3. 字体系统

**主字体：**
```
Inter Variable, "Noto Sans SC", "PingFang SC", system-ui, sans-serif
```

**等宽字体：**
```
"Berkeley Mono", ui-monospace, "SF Mono", Menlo, monospace
```

**字号层级：**
| 角色 | 字号 | 字重 | 行高 |
|------|------|------|------|
| 页面标题（H1） | 24px | 600 | 1.33 |
| 卡片标题（H2） | 18px | 600 | 1.33 |
| 正文大 | 16px | 400 | 1.5 |
| 正文 | 14px | 400 | 1.5 |
| 标签/小字 | 12px | 500 | 1.4 |
| 微型标签 | 11px | 500 | 1.3 |

## 4. 布局原则

### 4.1 侧边导航 + 内容区

```
+------------------+----------------------------------------+
|     侧边栏        |              内容区                     |
|    (260px 固定)    |         (自适应最大 1200px)            |
|                  |                                        |
|  [品牌标识]        |  [页面顶栏: 标题 + 描述]              |
|                  |                                        |
|  [导航菜单]        |  [页面内容]                            |
|                  |                                        |
|  [主题切换]        |                                        |
|                  |                                        |
|  [实例状态]        |                                        |
+------------------+----------------------------------------+
```

### 4.2 间距系统

基于 4px 基准：
- `space-1`：4px
- `space-2`：8px
- `space-3`：12px
- `space-4`：16px
- `space-5`：20px
- `space-6`：24px
- `space-7`：28px
- `space-8`：32px
- `space-9`：36px
- `space-10`：40px
- `space-12`：48px

### 4.3 圆角系统

- 微圆角（`radius-sm`）：4px（标签、小按钮）
- 标准圆角（`radius-md`）：6px（按钮、输入框、卡片）
- 大圆角（`radius-lg`）：8px（面板、对话框）
- 全圆角（`radius-full`）：9999px（胶囊标签）

### 4.4 响应式策略

- 桌面（> 960px）：侧边栏固定，内容区自适应
- 移动端（≤ 960px）：侧边栏折叠为顶部横条，内容区单列

## 5. 组件样式

### 5.1 卡片

**浅色模式：**
- 背景：`--surface-raised`（白色）
- 边框：`1px solid --border-default`
- 圆角：`radius-md`（6px）
- 阴影：`--shadow-sm`

**深色模式：**
- 背景：`--surface-raised`（#191a1b）
- 边框：`1px solid --border-subtle`
- 圆角：`radius-md`（6px）
- 阴影：无

### 5.2 按钮

**主按钮：**
- 背景：`--accent`
- 文字：`#ffffff`
- 悬浮：`--accent-hover`
- 圆角：`radius-md`
- 内边距：8px 16px

**次要按钮：**
- 背景：`transparent`
- 边框：`1px solid --border-default`
- 文字：`--text-primary`
- 悬浮：背景变为 `--surface-overlay`

**危险按钮：**
- 背景：`--status-danger-bg`
- 边框：`1px solid --status-danger`
- 文字：`--status-danger`

### 5.3 状态标签

使用 TinyVue `<Tag>` 组件，颜色映射：
- success：`--status-success` 背景 + 文字
- danger：`--status-danger-bg` 背景 + `--status-danger` 文字
- warning：`--status-warning-bg` 背景 + `--status-warning` 文字
- info：`--status-info-bg` 背景 + `--status-info` 文字

### 5.4 表单

使用 OpenTiny 表单组件，通过 CSS 变量覆盖默认样式：
- 输入框高度：36px
- 输入框背景：`--input-bg`
- 输入框边框：`--border-default`，悬浮/聚焦：`--accent`
- 标签文字：`--text-secondary`，字号 12px

## 6. 页面设计

### 概览页（/）
- 页面标题："实例概览"
- 6 个指标卡片（3x2 网格）：实例名称、提供方数量、模板数量、MCP 服务数量、就绪 MCP 数量、默认提供方
- 指标卡片：白色背景（浅色）/ 抬起表面（深色），带图标、标签、数值

### 健康检查（/health）
- 顶部状态横幅：绿色/黄色/红色圆点 + 状态文字 + 实例名称
- 3 列指标网格：提供方数量、模板数量、就绪 MCP 数量

### 运行状态（/runtime）
- 连接状态标签：EventSource 已连接 / 轮询降级
- 6 个指标卡片（2x3 网格）：活跃线程、运行中、排队中、已冷却、已驱逐、预估内存
- 运行健康诊断：队列压力、逐出 runtime、不可恢复 runtime、峰值内存
- 诊断建议：基于 snapshot 生成需关注/正常状态和处理提示
- 两个面板：线程池运行时、后台 Job 运行时

### 提供方管理（/providers）
- 左右分栏：列表（左侧）+ 表单（右侧）
- 列表：提供方卡片，显示名称、base URL、默认模型、是否默认标签
- 操作按钮：测试连接、编辑、删除
- 表单：创建/编辑提供方，支持测试连接

### 模板管理（/templates）
- 页面标题 + 数量标签
- 2 列卡片网格：模板名称、版本标签、描述
- 操作：删除（危险按钮风格）

### MCP 服务（/mcp）
- 顶部运维摘要：总服务、就绪服务、需关注、已发现工具
- 服务器列表卡片：名称、启用状态标签、传输目标、工具数量
- 诊断信息：超时、最近检查、最近成功、最近错误
- 操作：刷新、查看工具、测试连接、编辑、删除
- 展开视图：工具名称、描述与 Schema 预览
- 创建卡片：使用 OpenTiny 按钮式 tabs 在“手动配置”和“JSON 导入”之间切换
- JSON 导入：支持从 MCP JSON 配置片段批量导入 stdio 服务
- 手动配置：创建/编辑 MCP 服务，支持 `stdio` / `HTTP` / `SSE` 传输配置
- 表单操作：保存、重置/取消编辑、测试当前配置，反馈成功与错误状态

### 工具注册表（/tools）
- 顶部摘要：总工具、高风险及以上、Critical、Medium
- 工具卡片：工具名称、描述、风险等级标签
- 展开视图：参数 Schema 预览
- 不提供执行按钮，管理台只做运维可见性，不扩大工具执行面

### 系统设置（/settings）
- 单卡片布局
- 当前实例信息和默认提供方展示
- 表单：实例名称（输入框）、默认提供方（下拉选择）
- 保存按钮：加载状态、成功反馈、错误反馈

## 7. API 约定（不变）

前端保持现有 REST API 契约不变：
- `GET /api/v1/health` - 健康检查
- `GET /api/v1/bootstrap` - 实例引导信息
- `GET /api/v1/providers` - 提供方列表
- `POST /api/v1/providers` - 创建提供方
- `PATCH /api/v1/providers/:id` - 更新提供方
- `DELETE /api/v1/providers/:id` - 删除提供方
- `POST /api/v1/providers/:id/test` - 测试提供方连接
- `POST /api/v1/providers/test` - 测试提供方草稿配置
- `GET /api/v1/agents/templates` - 模板列表
- `POST /api/v1/agents/templates` - 创建模板
- `PATCH /api/v1/agents/templates/:id` - 更新模板
- `DELETE /api/v1/agents/templates/:id` - 删除模板
- `GET /api/v1/mcp/servers` - MCP 服务器列表
- `POST /api/v1/mcp/servers` - 创建 MCP 服务器
- `PATCH /api/v1/mcp/servers/:id` - 更新 MCP 服务器
- `DELETE /api/v1/mcp/servers/:id` - 删除 MCP 服务器
- `POST /api/v1/mcp/servers/:id/test` - 测试 MCP 服务器
- `POST /api/v1/mcp/servers/test` - 测试 MCP 服务器草稿配置
- `GET /api/v1/mcp/servers/:id/tools` - MCP 服务器工具列表
- `GET /api/v1/tools` - 工具注册表
- `GET /api/v1/settings` - 获取设置
- `PUT /api/v1/settings` - 更新设置
- `GET /api/v1/runtime` - 运行时快照
- `GET /api/v1/runtime/events` - 运行时事件流（SSE）

## 8. 技术约束

- **不改动 desktop**
- **不做 chat 界面**
- **不做 thread monitor 新能力**
- **不做 shared frontend core / packages/app-core**
- **不让 server 托管 web 静态资源**
- 前端使用 Vue 3 + OpenTiny Vue + Vite
- 优先使用 OpenTiny Vue 组件和 token 覆盖
- 主题切换通过 CSS class + localStorage 实现
