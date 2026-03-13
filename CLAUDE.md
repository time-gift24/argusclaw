# ArgusClaw 开发指南

## 构建与测试

```bash
prek                                           # 静态检查基线
                                               # - git commit 时会自动运行检查，禁止跳过
                                               # - fmt 问题会自动修复，无需改动再次提交
                                               # - clippy 相关问题务必做修复
cargo deny check                               # 发起 PR 前使用，检测下静态基线
RUST_LOG=argusclaw=debug,claw=debug cargo run  # 开启日志运行
```

## 设计与检视原则(非常重要)
- YAGNI（You Ain't Gonna Need It，你不会需要它）
- KISS (Keep It Simple and Stupid，尽可能保持简单)
- DRY (Don't Repeat Yourself, 禁止重复你自身)

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

## 项目结构

cli 和 desktop 都依赖同一个 claw 启动项 AppContext，及 cli 和 desktop 只能看到 AppContext 一个结构体 (极其重要)


```text
crates/
├── claw/
│   ├── src/
│   │   ├── lib.rs                    # 库入口，模块声明和导出
│   │   ├── error.rs                  # 顶层错误类型
│   │   ├── claw.rs                   # AppContext；拥有 LLMManager、AgentManager
│   │   ├── agents/                   # Agent 管理
│   │   │   ├── mod.rs                # AgentManager (占位)
│   │   │   ├── thread/               # Thread 多轮对话模块
│   │   │   │   ├── mod.rs            # Thread 模块入口和重导出
│   │   │   │   ├── config.rs         # ThreadConfig, CompactStrategy
│   │   │   │   ├── error.rs          # ThreadError 类型
│   │   │   │   ├── thread.rs         # Thread, ThreadBuilder, TurnStreamHandle
│   │   │   │   └── types.rs          # ThreadId, ThreadState, ThreadEvent
│   │   │   └── turn/                 # Turn 执行模块
│   │   │       ├── mod.rs            # Turn 模块入口和重导出
│   │   │       ├── config.rs         # TurnConfig, TurnInput, TurnOutput, TokenUsage
│   │   │       ├── error.rs          # TurnError 类型
│   │   │       ├── hooks.rs          # Hook 系统 (HookEvent, HookHandler, HookRegistry)
│   │   │       └── execution.rs      # execute_turn 及并行工具支持
│   │   ├── db/                       # 存储抽象和实现
│   │   │   ├── mod.rs                # DB 模块入口和共享错误
│   │   │   ├── llm.rs                # LLM 提供商记录和仓库 trait
│   │   │   ├── thread.rs             # Thread 持久化类型和 ThreadRepository trait
│   │   │   └── sqlite/               # SQLx 支持的 SQLite 实现
│   │   │       ├── mod.rs            # SQLite 连接和迁移辅助
│   │   │       ├── llm.rs            # SQLite LLM 提供商仓库
│   │   │       └── thread.rs         # SQLite Thread 仓库
│   │   ├── llm/                      # LLM 领域类型、管理器和提供商实现
│   │   │   ├── mod.rs                # LLM 模块入口和导出
│   │   │   ├── error.rs              # 提供商无关的 LLM 错误
│   │   │   ├── manager.rs            # LLMManager：列出提供商和构建实例
│   │   │   ├── provider.rs           # 核心 LlmProvider trait 和请求/响应类型
│   │   │   ├── retry.rs              # LlmProvider 重试包装器
│   │   │   ├── secret.rs             # 主机绑定的 API 密钥加密/解密
│   │   │   └── providers/            # 具体提供商实现
│   │   │       ├── mod.rs            # 提供商模块导出
│   │   │       └── openai_compatible.rs # OpenAI 兼容提供商工厂和实现
│   │   ├── tool/                     # Agent/LLM 工具注册表
│   │   │   └── mod.rs                # NamedTool trait、ToolManager、ToolError
│   │   ├── job/                      # Job 调度领域模型
│   │   │   ├── mod.rs                # 模块入口
│   │   │   ├── types.rs              # JobId, JobType, JobRecord
│   │   │   ├── error.rs              # JobError
│   │   │   └── repository.rs         # JobRepository trait
│   │   ├── scheduler/                 # Scheduler 运行时
│   │   │   ├── mod.rs                # 模块入口
│   │   │   ├── config.rs             # SchedulerConfig
│   │   │   ├── error.rs              # SchedulerError
│   │   │   └── scheduler.rs          # Scheduler 核心实现
│   │   ├── workflow/                 # Workflow 领域模型（轻量分组）
│   │   │   ├── mod.rs                # 模块入口
│   │   │   ├── types.rs              # WorkflowId, JobId, WorkflowStatus
│   │   │   └── repository.rs         # WorkflowRepository trait
│   │   └── api/                      # GraphQL API 层
│   │       ├── mod.rs                # Schema 构建器
│   │       ├── types.rs              # Workflow/Stage/Job GraphQL 类型
│   │       ├── query.rs              # Query resolvers
│   │       └── mutation.rs           # Mutation resolvers
│   ├── migrations/                   # SQLx 迁移
│   └── tests/                        # E2E 测试；不适合内联测试的多模块场景
│       ├── turn_integration_test.rs  # Turn 模块集成测试
│       ├── thread_integration_test.rs # Thread 模块集成测试
│       └── thread_repository_test.rs # Thread SQLite 持久化测试
├── desktop/                          # Tauri + React + TypeScript + Vite + Tailwind CSS v4
│   ├── src/                         # React 前端
│   └── src-tauri/                    # Rust 后端
└── cli/
    ├── CLAUDE.md                      # CLI 模块指南
    └── src/
        ├── main.rs                    # CLI 引导：tracing、DB 初始化、AppContext 启动
        ├── dev.rs                     # 开发-only 命令 (behind `dev` feature)
        └── dev/
            └── config.rs              # 提供商导入 TOML 格式
```

## 测试

- 优先使用 `#[cfg(test)]` 在实现文件中的内联测试
- 只在需要测试多个模块组合的 E2E 场景使用 `crates/*/tests/`

## 数据库

- 默认 `DATABASE_URL` 为 `~/.argusclaw/sqlite.db`
- 使用 `sqlx::migrate!()` 宏，迁移在**编译时嵌入**到二进制文件中

### 迁移规范

- 文件位于 `crates/claw/migrations/` 目录
- **使用 sqlx-cli 创建迁移**：`sqlx migrate add <name>`（在 `crates/claw` 目录下执行）
- **并发开发注意**：多个 feature 分支并发开发时，rebase origin/main 后必须检查迁移文件时间戳顺序，必要时重新命名以保障时序正确

## 工具模块

- `NamedTool` trait：`name()`、`definition()`、`execute()` — 用于 Agent/LLM 工具抽象
- `ToolManager`：基于 DashMap 的注册表，包含 `register()`、`get()`、`list_definitions()`、`execute()`
- 复用 `llm/provider.rs` 中的 `ToolDefinition`（单一事实来源）

## Turn 模块

- 单次 turn 执行（LLM → Tool → LLM 循环），支持并行工具调用
- `TurnConfig`：配置 max_tool_calls、tool_timeout_secs、max_iterations
- `TurnInput`：messages、system_prompt、provider、tool_manager、tool_ids、hooks
- `TurnOutput`：更新后的消息历史和 token 使用统计
- `TurnError`：LLM 失败、工具执行、hooks、限制等错误类型
- `HookRegistry`：可扩展的 hook 系统
- `execute_turn()`：turn 执行的主入口

## Workflow 模块

- 领域模型：Workflow 作为轻量分组，Job 通过 depends_on 表达依赖关系
- `WorkflowRepository` trait：定义 Workflow CRUD 操作
- 状态：Pending、Running、Succeeded、Failed、Cancelled

## Job 模块

- 通用 Job 模型：standalone（独立）、workflow（流水线）、cron（定时）
- `JobRepository` trait：定义 Job CRUD 操作
- `JobType` 枚举：Standalone、Workflow、Cron
- `depends_on`：JSON 数组存储 Job 依赖关系

## Scheduler 模块

- 轮询模式调度：固定间隔检查 pending jobs
- 依赖检查：确保 depends_on 全部 succeeded 才调度
- Cron 支持：定时创建 standalone job
- 并发控制：max_concurrent_jobs 限制

## API 模块

- 使用 async-graphql 实现 GraphQL schema
- Query：workflow、workflows
- Mutation：create_workflow、add_job、update_job_status
- 通过 Tauri Commands 暴露给前端

## Thread 模块

- 多轮对话会话管理，封装消息历史并顺序执行 Turn
- `ThreadId`：强类型 UUID 包装器
- `ThreadState`：`Idle` | `Processing`
- `ThreadEvent`：Processing/TurnCompleted/TurnFailed/Idle/Compacted 事件（broadcast channel）
- `ThreadConfig`：compact_threshold_ratio、turn_config
- `Compactor` trait：async `compact(&mut Thread)` 方法，负责判断和执行压缩
- `KeepRecentCompactor`：保留最近 N 条消息
- `KeepTokensCompactor`：按 token 预算保留消息
- `CompactorManager`：管理 compactor 实例
- `Thread`：核心结构，包含 messages、token_count、turn_count、event_sender、compactor
- `ThreadBuilder`：使用 derive_builder 构建 Thread
- `send_message()`：发送用户消息并执行 Turn
- `subscribe()`：订阅 Thread 事件（CLI、Tauri）
- `ThreadRepository` trait：持久化抽象（upsert_thread、get_messages、delete_messages_before 等）
- `SqliteThreadRepository`：SQLite 实现，支持 CASCADE 删除

## Approval 模块

工具执行前的审批确认系统，通过 Hook 机制集成到 Turn 执行流程。

### 核心组件

- `ApprovalPolicy`：定义哪些工具需要审批、超时时间、自动批准策略
- `ApprovalRequest`：审批请求，包含 agent_id、tool_name、action_summary、risk_level
- `ApprovalResponse`：审批响应，包含 decision (Approved/Denied/TimedOut)
- `ApprovalManager`：管理审批请求的生命周期，提供 `request_approval()` 和 `resolve()` 方法
- `ApprovalHook`：实现 `HookHandler` trait，在 `BeforeToolCall` 事件中拦截需要审批的工具

### 工作流程

1. `ApprovalHook` 在 `BeforeToolCall` 时检查 `ApprovalPolicy`
2. 如果工具需要审批，创建 `ApprovalRequest` 并发送 `ThreadEvent::WaitingForApproval`
3. 等待用户或系统响应（批准/拒绝/超时）
4. 发送 `ThreadEvent::ApprovalResolved` 并返回 `HookAction::Continue` 或 `Block`

### CLI 集成

- `--approval-tools`：指定需要审批的工具列表（逗号分隔）
- `--auto-approve`：自动批准所有审批请求（用于测试）

### 内置工具

- `ShellTool`：执行 shell 命令，`RiskLevel::Critical`，默认需要审批
  - 参数：`command`（必需）、`timeout`（可选，默认 120s）、`cwd`（可选）
  - 返回：`stdout`、`stderr`、`exit_code`
