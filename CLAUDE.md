# ArgusWing 开发指南

## 构建与测试
务必使用 `cargo install prek && prek install` 初始化项目

```bash
prek                                           # 静态检查基线
                                               # - git commit 时会自动运行检查，禁止跳过
                                               # - fmt 问题会自动修复，无需改动再次提交
                                               # - clippy 相关问题务必做修复
cargo deny check                               # 发起 PR 前使用，检测下静态基线
RUST_LOG=arguswing=debug,argus=debug cargo run  # 开启日志运行
```

## 设计与检视原则(非常重要)
- YAGNI（You Ain't Gonna Need It，你不会需要它）
- KISS (Keep It Simple and Stupid，尽可能保持简单)
- DRY (Don't Repeat Yourself，禁止重复你自身)

## 人机交互第一性原则（非常重要）
- 动机与目标务必澄清，禁止假设我清楚我的目标
- 动机与目标明晰后，客观给予我更优的最短路径实现
- 出现任何设计缺陷和bug等，禁止以补丁思维说明，而是追溯到最源头的动机解释根因
- 输出保持简洁，直击变更点或重点，禁止复述不变部分

## 编码前检查

**禁令（极其重要）**
- ❌ 禁止直接在 `main` 分支的文件夹中修改代码
- ❌ 禁止直接在 `main` 分支创建或修改文件
- ✅ 必须始终在 `.worktrees/` 中的某个独立分支工作

使用 `using-git-worktrees` skill 创建独立工作区：

```bash
# 创建新功能分支
.worktrees/feature-xxx  # 在这里工作
```

## 分支与文档规则

- **docs/** 目录：始终放在 `main` 分支，不随功能分支
- **清理分支时**：同时删除该分支关联的 docs/ 目录
- **各 crate 特性**：一句话描述放在对应 crates/*/CLAUDE.md 顶部

## 提交规则

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


## 持久化层隔离

- 禁止在非 argus-repository 中编写任何 sql 语句
- 所有需要依赖持久化的部分均使用 Arc<dyn > 依赖注入，而不是依赖具体实现

## Crate 依赖图

```
                            ┌─────────────────────────────────────────────────────────┐
                            │                        cli                              │
                            │                    (命令行前端)                            │
                            └──────────────────────┬──────────────────────────────────┘
                                               │ 仅通过公共 API 依赖 argus-wing
                                               ▼
┌─────────────┐    ┌──────────────────────────────────────────────────────────────────────────┐
│   desktop   │    │                              argus-wing                                │
│  (Tauri 前端)│    │                         (核心库门面)                                      │
└──────┬──────┘    └──────────────────────────────────┬───────────────────────────────────────────┘
       │                      ▲                         │
       │                      │ 仅通过公共 API 依赖        │ 内部模块
       ▼                      │                         │
                              │            ┌────────────┴────────────┐
                              │            │                         │
┌─────────────────┐    ┌──────┴──────────┐│                        ┌┴──────────────────┐
│argus-protocol   │◄───│  argus-session  ││                        │ argus-agent       │
│  ★ 核心类型 ★    │    │   会话管理      ││                        │  智能体(线程+轮次) │
│                 │    └─────────────────┘│                        └───────────────────┘
│ • ThreadId      │             ▲          │
│ • ThreadEvent ★ │             │          │
│ • TokenUsage    │             └──────────┴──────────────┐
│ • Approval*     │                        │              ┌───────────────────┐
│ • RiskLevel     │             ┌───────────┴───────────┐  │ argus-llm         │
│ • Hook*         │             │                       │  └───────────────────┘
│ • LlmProvider   │    ┌────────┴───────┐    ┌─────────┴───────┐
│ • NamedTool     │    │argus-approval │    │ argus-tool       │
└─────────────────┘    │  审批系统       │    │  工具注册表       │
        ▲              └────────────────┘    └──────────────────┘
        │
        │    ┌────────────────┐        ┌──────────────────┐
        ├────┤ argus-job      │        │ argus-template   │
        │    │  后台任务      │        │  模板            │
        │    └────────────────┘        └──────────────────┘
        │
        │    ┌────────────────┐
        ├────┤ argus-test-support │
        │    │  测试辅助       │
        │    └────────────────┘
        │
        │    ┌────────────────┐
        ├────┤ argus-auth      │
        │    │  账号认证       │
        │    └────────────────┘
        │
        │    ┌────────────────┐
        └────┤ argus-crypto    │
             │  加密          │
             └────────────────┘
```

**叶子 crate**（无内部依赖）：`argus-protocol`、`argus-auth`、`argus-crypto`、`argus-test-support`

## 核心规则

**desktop 只依赖 argus-wing 暴露的公共 API，不可访问 argus-* 内部模块。

## argus-protocol — 核心类型（叶子模块）

`argus-protocol` 是整个项目的**核心类型库**，不依赖其他 argus-* crates（仅依赖外部 crate 如 serde、uuid、chrono、thiserror）。

它存在的主要目的：
1. **打破循环依赖**：`agents` ↔ `approval` ↔ `tool` 之间不能直接相互依赖
2. **提供核心 trait**：`LlmProvider`、`NamedTool`、`HookHandler`、`ProviderResolver`
3. **定义共享类型**：`ThreadId`、`ThreadEvent`、`TokenUsage`、`Approval*`、`RiskLevel`

### ThreadEvent — 核心事件总线 ★

`ThreadEvent` 是整个应用的事件总线，所有层通过它向上传播状态：

```rust
pub enum ThreadEvent {
    Processing { thread_id, turn_number, event: LlmStreamEvent }, // LLM/工具流式事件
    ToolStarted { thread_id, turn_number, tool_call_id, tool_name, arguments },
    ToolCompleted { thread_id, turn_number, tool_call_id, tool_name, result },
    TurnCompleted { thread_id, turn_number, token_usage },
    TurnFailed { thread_id, turn_number, error },
    Idle { thread_id },                    // 线程进入空闲
    Compacted { thread_id, new_token_count }, // 上下文被压缩
    WaitingForApproval { thread_id, turn_number, request },
    ApprovalResolved { thread_id, turn_number, response },
    JobCompleted { job_id, status, session_id, message }, // 后台任务完成
}
```

**事件流向**：
```
Turn (stream_tx: TurnStreamEvent)
  → Event Forwarder Task
  → thread_event_tx: ThreadEvent
  → Thread → Session → ArgusWing → Desktop/CLI
```

### 其他核心类型

- `ThreadId` / `SessionId` / `AgentId` / `ProviderId`：强类型 ID 包装器
- `TokenUsage`：token 使用统计
- `ApprovalDecision` / `ApprovalRequest` / `ApprovalResponse`：审批协议类型
- `RiskLevel`：操作风险等级（Low / Medium / High / Critical）
- `HookEvent` / `HookHandler` / `HookRegistry`：生命周期 Hook 系统
- `LlmProvider` trait：LLM 提供者抽象
- `NamedTool` trait：工具抽象

依赖方向：`argus-protocol` ← 所有其他 argus-* crates（**argus-protocol 不依赖它们**）

## argus-wing — 核心库门面

`argus-wing` 是面向 desktop 的**唯一入口点**。它不包含核心逻辑，而是组合各个 argus-* 模块。

唯一的入口点是 `ArgusWing::init()` / `ArgusWing::with_pool()`。desktop 只能看到 `ArgusWing` 一个结构体来启动和操作系统。

## 各 argus-* crate 职责

| Crate | 职责 | 关键依赖 |
|-------|------|---------|
| `argus-protocol` | 核心类型定义（叶子模块） | 无内部依赖 |
| `argus-agent` | 统一智能体：Turn 执行引擎 + Thread 会话管理 | protocol, tool, llm, test-support |
| `argus-session` | 会话管理 | protocol, template, agent, job |
| `argus-llm` | LLM 抽象层 | protocol, test-support, crypto |
| `argus-approval` | 审批系统 | protocol |
| `argus-tool` | 工具注册表 | protocol |
| `argus-repository` | 持久化层 | protocol, llm |
| `argus-template` | 模板 | protocol |
| `argus-job` | 后台任务管理 | protocol, template, tool |
| `argus-auth` | 账号认证（叶子模块） | protocol, crypto |
| `argus-crypto` | 加密（叶子模块） | 无内部依赖 |
| `argus-test-support` | 测试辅助（叶子模块） | protocol |

## cli — 命令行前端

详见 `crates/cli/CLAUDE.md`。
