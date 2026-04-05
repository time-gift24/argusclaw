# 删除当前 Approval 逻辑并重建设计文档

## 背景

当前仓库存在一整套 approval 相关结构：

- 独立的 `crates/argus-approval`
- `argus-protocol` 中的 approval 类型与线程事件
- `argus-wing` 暴露的 approval API
- `argus-agent` 中等待 approval 的 turn/thread 进度与运行态
- `desktop` 中的 approval UI、store 状态、Tauri command 和事件映射

但经过代码排查，当前默认 thread 构建路径里并没有看到 `ApprovalHook` 的生产接线。现状更接近“未完成的旧骨架”而不是“正在工作中的生产能力”。

这意味着现在最合理的策略不是补修旧 approval，而是先把整套 approval 面删除，恢复一个干净基线，再基于真实需求重新设计。

## 目标

本轮仅做一件事：

- 彻底移除现有 approval 逻辑，包括后端、协议、线程状态、Tauri 绑定、前端 UI 和对应测试。

本轮不做的事：

- 不引入新的审批/授权/风险拦截方案
- 不重新命名旧 approval 并做兼容封装
- 不保留“等待人工动作”的旧中断骨架

## 设计决策

### 1. 采用“纯删除”而不是保留兼容骨架

用户已明确选择纯删除。

原因：

- 当前 approval 生产接线不完整，保留旧 seam 只会继续污染主路径
- 删除后系统行为更容易理解：工具直接执行，不再存在等待批准的隐藏分叉
- 新设计可以从真实需求出发，不被 `ApprovalRequest` / `ApprovalResolved` / `resolve_approval` 这些旧抽象绑死

### 2. 保留 `RiskLevel`

`RiskLevel` 不随 approval 一起删除。

原因：

- 它已经被 `NamedTool::risk_level()` 和多个工具实现广泛使用
- 它现在表达的是“工具风险元数据”，不是“审批系统专属类型”
- 下一版如果要重新做权限、策略、确认、审计或 UI 提示，`RiskLevel` 仍然是有价值的基础能力

### 3. 删除等待/恢复事件，而不是保留成通用中断协议

本轮会删除：

- `ThreadEvent::WaitingForApproval`
- `ThreadEvent::ApprovalResolved`
- `TurnProgress::WaitingForApproval`
- `TurnProgress::ApprovalResolved`
- `ThreadRuntimeState::WaitingForApproval`

原因：

- 当前唯一语义就是 approval；硬改成“通用人工动作”只会制造一个没有消费者的新抽象
- 前端和 Tauri 端也已经围绕 approval 命名展开，继续保留只会产生半残状态面

## 实施边界

### Rust Core

删除整个 `crates/argus-approval` crate，并移除：

- workspace member
- `argus-wing` 对它的依赖和实例化
- `argus-wing` 暴露的 approval API
- 相关测试与注释

### Protocol

删除 `argus-protocol/src/approval.rs` 模块及其导出，并同步删除 approval 事件变体。

保留：

- `risk_level.rs`
- `NamedTool::risk_level()`

### Agent Runtime

从 `argus-agent` 删除所有 approval 相关运行态、进度映射和测试辅助 hook，让 turn/thread 只保留正常执行路径。

### Desktop / Tauri

删除：

- `ApprovalPrompt` 组件
- `pendingApprovalRequest` store 状态
- pending assistant 的 `requires-action` 中断态
- `resolve_approval` Tauri command
- approval 事件 payload
- 前端 approval 相关测试

## 数据流变化

删除前：

1. 工具调用前可能生成 approval request
2. thread/turn 进入 waiting 状态
3. Tauri 把 approval 事件转给前端
4. 前端显示批准/拒绝 UI
5. UI 调用 `resolve_approval`
6. runtime 恢复继续执行

删除后：

1. 工具调用直接执行
2. thread/turn 只经历正常的 running/completed/failed 生命周期
3. 前端不再展示 approval UI，也不再持有 approval 中间状态

## 风险与回退

### 已知风险

- 可能有少量文档、注释或测试遗漏引用 approval 术语
- `argus-agent` 的 approval 相关测试删除后，需要确认 turn progress 其他路径未被意外影响
- `desktop` 的测试快照/断言可能因为状态面收缩需要同步更新

### 风险控制

- 先删协议和公开 API，再清理调用点，避免悬空引用
- 用编译错误驱动剩余清理
- 分别运行 Rust 与 desktop 的定向验证

### 回退策略

如果删除过程发现某条生产链路实际上依赖 approval：

- 不恢复旧 UI
- 先让该路径回到“直接执行”主线
- 仅在必要时做最小修复，避免重新引入 approval 语义

## 验证方案

Rust 侧：

- `cargo test -p argus-protocol`
- `cargo test -p argus-agent`
- `cargo test -p argus-wing`

Desktop 侧：

- `pnpm test` in `crates/desktop`
- 如有必要补充 `pnpm build`

完成标准：

- workspace 中不存在 approval crate 及其依赖
- thread event / turn progress / desktop store 不再含 approval 状态
- 聊天主路径编译通过，相关测试通过

## 下一版重设计留白

本轮删除完成后，下一版可以从零讨论以下问题，而不是继承旧 approval 设计：

- 是否真的需要“阻塞式人工批准”
- 是否只对部分高风险工具做确认
- 是否区分“确认”“授权”“审计”“策略”这几个概念
- 是否把风险提示放在 UI 展示层，而不是 turn 执行层
- 是否需要更通用的人机协同中断模型
