# Argus Web

> 特性：Vue 3 + OpenTiny Vue + Vite 独立中文管理台与 Web 对话页。

## 作用域

- 本文件适用于 `apps/web/` 及其子目录。
- 视觉、交互、页面结构与 API 契约优先参考 `apps/web/DESIGN.md`。

## 已归档阶段上下文

- Phase 1：交付独立 Web 管理台，覆盖 `health`、`bootstrap`、providers、templates、MCP、tools 的可用管理闭环。
- Phase 2：接入 runtime snapshot 与 runtime SSE，事件流失败时保留轮询/刷新降级。
- Phase 3：保持 `apps/web` 与 desktop 独立，不抽 `packages/app-core`，不复用 desktop store。
- Phase 4：增强 MCP / tools 运维可见性，MCP 支持手动配置、JSON 导入、测试连接、删除、工具发现。
- Phase 5：新增独立 `/chat` 页面，基于 server REST/SSE API，不使用 desktop chat store。
- Phase 6：修正首发对话可用性，包括草稿会话名、空名称兜底、工具调用摘要和失败回滚。
- Phase 7：在 Web 对话中展示当前 turn 的工具、重试、失败等运行活动。
- Phase 8：将对话展示映射、运行活动、消息舞台、对话面板拆出组件/composable，避免 `ChatPage` 继续膨胀。
- Phase 9：新增 `/agent-runs` 页面，调用 server-only agent run REST API 触发指定智能体运行并查询最近一次状态。

## 核心职责

- 作为独立 Vite 应用运行，不由 `argus-server` 托管静态资源。
- 使用 Vue 3、Vue Router、TypeScript、Vitest 和 OpenTiny Vue。
- 管理台页面覆盖实例概览、健康检查、运行状态、模型提供方、智能体模板、MCP 服务、工具注册表、Agent Runs 和 Web 对话。
- 所有业务数据通过 `src/lib/api.ts` 调用 `argus-server` REST/SSE API；server snapshot/messages 是事实来源。
- `/chat` 使用 TinyRobot 作为消息与输入区基础，管理控件仍优先使用 OpenTiny Vue。

## 修改守则

- 不改 desktop，不引入 desktop store，不新增 shared frontend core / `packages/app-core`。
- 不新增登录、多用户或系统设置页面；`settings/admin_settings` 已明确移除。
- 优先使用 OpenTiny Vue 组件和 token 覆盖，不重造基础按钮、输入框、选择器、开关等组件。
- 页面文字保持中文优先，术语与 `DESIGN.md` 保持一致。
- 管理页面采用读写分离：列表/详情页负责读取与快速操作，新增/编辑/导入走独立路由并使用面包屑。
- MCP 创建入口优先 JSON 导入；手动配置和 JSON 导入应共享创建/测试配置能力，不复制业务逻辑。
- Chat SSE 只用于 live delta 和运行活动展示；settled 后必须刷新 REST snapshot/messages。
- 发送失败时必须回滚未落库的乐观消息；切换 session/thread 时必须清理 pending stream/activity 状态。
- OpenTiny `Select` 的 model value 避免传入会触发运行时警告的裸数字，必要时在组件边界做 string 转换。

## 设计守则

- 默认浅色，支持深色主题切换；主题状态由根元素 class 和 localStorage 管理。
- 保持传统管理台布局：左侧导航 + 右侧内容区，移动端折叠为单列/顶部导航。
- 对话页参考 opencode desktop / Codex 对话风格：轻量上下文入口、主消息舞台、底部 composer、单个 pending assistant bubble 流式累积。
- 空状态、加载、错误、保存成功、测试失败等都必须有中文反馈，不能让 404/502 等错误变成未处理 rejection。

## 验证

常用验证：

```bash
cd apps/web && pnpm install
cd apps/web && pnpm exec vitest run
cd apps/web && pnpm build
```

涉及单页或 composable 时，先跑对应的 targeted Vitest，再跑全量 Vitest。
