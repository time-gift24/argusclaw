# ArgusWing 开发指南

## 构建与测试
务必使用 `cargo install prek && prek install` 初始化项目

```bash
prek                                           # 静态检查基线
                                               # - git commit 时会自动运行检查，禁止跳过
                                               # - fmt 问题会自动修复，无需改动再次提交
                                               # - clippy 相关问题务必做修复
cargo deny check                               # 发起 PR 前使用，检测下静态基线
RUST_LOG=arguswing=debug,claw=debug cargo run  # 开启日志运行
```

## 设计与检视原则(非常重要)
- YAGNI（You Ain't Gonna Need It，你不会需要它）
- KISS (Keep It Simple and Stupid，尽可能保持简单)
- DRY (Don't Repeat Yourself, 禁止重复你自身)

## 编码前检查
- 禁止在 main (极其重要) 分支工作，如果在 main 分支则使用 using-git-worktrees 去独立分支工作
- 完成工作后无需提问直接发起 PR

## 代码风格

- 跨模块导入使用 `crate::`；测试和模块内引用使用 `super::`
- 不使用 `pub use` 重导出，除非是暴露给下游消费者
- 生产代码中不使用 `.unwrap()` 或 `.expect()`（测试中可以使用）
- 错误类型使用 `thiserror` 定义在 `error.rs` 中
- 错误映射添加上下文：`.map_err(|e| SomeError::Variant { reason: e.to_string() })?`
- 优先使用强类型而非字符串（枚举、新类型）
- 保持函数职责单一，逻辑复用时提取辅助函数
- 只在逻辑不明显时添加注释

## 架构

优先使用通用/可扩展的架构，而非硬编码特定集成。实现前请先询问期望的抽象层次。

可扩展性关键 trait：Database、Channel、NamedTool、LlmProvider

所有 I/O 使用 tokio 异步。使用 Arc<T> 共享状态，RwLock 并发访问。

## Crate 关系（极其重要）

```text
crates/
├── claw/          # 核心库：所有业务逻辑
├── cli/           # CLI 前端
└── desktop/       # Tauri 桌面前端（React + Rust）
```

**核心规则：cli 和 desktop 都只依赖 claw 暴露的公共 API，不可访问 claw 内部模块。**

### claw — 核心库(迁移中，最终要废弃掉)

唯一的入口点是 `AppContext`（定义在 `claw.rs`）。cli 和 desktop 只能看到 `AppContext` 一个结构体来启动和操作系统。

对话流程通过 `Agent` API 暴露：`AgentBuilder::build()` → `agent.create_thread()` → `agent.send_message()` / `agent.subscribe()` / `agent.resolve_approval()`。内部的 Thread、Turn、Compact 模块对消费者不可见（`pub(crate)`，仅 `dev` feature 下为 `pub`）。

### protocol — 跨模块共享类型（claw 内部）

`protocol/` 是 claw 内部的**叶子模块**，不依赖其他 claw 模块（仅依赖外部 crate 如 serde、uuid、chrono）。它存在的目的是打破 `agents` ↔ `approval` ↔ `tool` 之间的循环依赖。

包含以下共享类型：
- `ThreadId`：强类型 UUID 包装器
- `ThreadEvent`：线程生命周期事件（Processing/TurnCompleted/TurnFailed/Idle/Compacted/WaitingForApproval/ApprovalResolved）
- `TokenUsage`：token 使用统计
- `ApprovalDecision` / `ApprovalRequest` / `ApprovalResponse`：审批协议类型
- `RiskLevel`：操作风险等级
- `HookEvent` / `HookHandler` / `HookRegistry`：生命周期 Hook 系统

这些类型通过 `claw::lib.rs` 的 `pub use protocol::{ ... }` 重导出给 cli/desktop 消费。

依赖方向：`protocol` ← `agents`、`approval`、`tool`、`scheduler`（**protocol 不依赖它们**）

### cli — 命令行前端

详见 `crates/cli/CLAUDE.md`。