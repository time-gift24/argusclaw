# Claw 核心库

## 项目结构

```text
src/
├── lib.rs                          # 库入口，模块声明和公共 API 重导出
├── error.rs                        # 顶层 AgentError
├── claw.rs                         # AppContext：拥有 LLMManager、AgentManager
├── protocol/                       # 跨模块共享类型（叶子模块，无内部依赖）
│   ├── thread_id.rs                # ThreadId
│   ├── thread_event.rs             # ThreadEvent
│   ├── token_usage.rs              # TokenUsage
│   ├── approval.rs                 # ApprovalDecision/Request/Response
│   ├── risk_level.rs               # RiskLevel
│   └── hooks.rs                    # HookEvent, HookHandler, HookRegistry
├── agents/                         # Agent 管理（pub(crate)，dev feature 下 pub）
│   ├── agent/                      # Agent 运行时
│   │   ├── runtime.rs              # Agent, AgentBuilder（字段私有，通过方法访问）
│   │   └── manager.rs              # AgentManager：创建/管理 Agent 实例
│   ├── thread/                     # Thread 多轮对话
│   │   ├── thread.rs               # Thread, ThreadBuilder（字段私有）
│   │   ├── config.rs               # ThreadConfig
│   │   ├── error.rs                # ThreadError, CompactError
│   │   └── types.rs                # ThreadInfo, ThreadState
│   ├── turn/                       # Turn 单次执行
│   │   ├── execution.rs            # execute_turn(), execute_turn_streaming()
│   │   ├── config.rs               # TurnConfig, TurnInput, TurnOutput
│   │   ├── error.rs                # TurnError
│   │   └── hooks.rs                # Hook 系统（重导出自 protocol）
│   ├── compact.rs                  # Compactor trait, KeepRecentCompactor, KeepTokensCompactor
│   └── types.rs                    # AgentId, AgentRecord, AgentRepository
├── approval/                       # 审批系统
│   ├── approval_hook.rs            # ApprovalHook（HookHandler 实现）
│   ├── manager.rs                  # ApprovalManager
│   ├── policy.rs                   # ApprovalPolicy
│   └── types.rs                    # 内部类型（重导出自 protocol）
├── llm/                            # LLM 提供商抽象
│   ├── provider.rs                 # LlmProvider trait, 请求/响应类型, ToolDefinition
│   ├── manager.rs                  # LLMManager
│   ├── error.rs                    # LlmError
│   ├── retry.rs                    # 重试包装器
│   ├── secret.rs                   # API 密钥加密/解密
│   └── providers/
│       └── openai_compatible.rs    # OpenAI 兼容提供商
├── tool/                           # 工具注册表
│   ├── mod.rs                      # NamedTool trait, ToolManager, ToolError
│   ├── shell.rs                    # ShellTool（RiskLevel::Critical）
│   ├── read.rs                     # ReadTool
│   ├── grep.rs                     # GrepTool
│   └── glob.rs                     # GlobTool
├── db/                             # 存储抽象和 SQLite 实现
│   ├── llm.rs                      # LlmProviderRepository trait
│   ├── thread.rs                   # ThreadRepository trait
│   └── sqlite/                     # SQLx SQLite 实现
├── job/                            # Job 调度领域模型
│   ├── types.rs                    # JobId, JobType, JobRecord
│   └── repository.rs              # JobRepository trait
├── scheduler/                      # Scheduler 运行时（轮询调度）
├── workflow/                       # Workflow 领域模型（轻量分组）
└── api/                            # GraphQL API 层（async-graphql）
tests/                              # E2E 多模块集成测试
migrations/                         # SQLx 迁移文件
```

## Agent 模块

Agent 是对外暴露的对话 API，封装了 Thread 和 Turn 的内部实现。

- `Agent`：管理多个 Thread，共享 provider/tool_manager/compactor/hooks
- `AgentBuilder`：构建 Agent，`build()` 返回 `Result<Agent, AgentError>`
- `AgentManager`：管理多个 Agent 实例，提供 passthrough 方法
- Agent 和 Thread 的**字段均为私有**，通过方法访问，确保封装性

## Thread 模块

- Thread 管理消息历史，顺序执行 Turn，通过 broadcast channel 广播事件
- `ThreadBuilder::build()` 返回 `Result<Thread, ThreadError>`
- `send_message()` → compact → execute_turn_streaming → 广播 ThreadEvent
- 对消费者不可见（`pub(crate)`）

## Turn 模块

- 单次 LLM → Tool → LLM 循环，支持并行工具调用
- `TurnInputBuilder::build()` 返回 `Result<TurnInput, TurnError>`
- `TurnConfig`：max_tool_calls、tool_timeout_secs、max_iterations

## Approval 模块

- 工具执行前的审批系统，通过 Hook 机制集成到 Turn 流程
- `ApprovalHook` 在 `BeforeToolCall` 时检查 `ApprovalPolicy`
- 审批请求通过 `ThreadEvent::WaitingForApproval` 通知消费者
- 消费者调用 `agent.resolve_approval()` 完成审批

## 工具模块

- `NamedTool` trait：`name()`、`definition()`、`execute()`
- `ToolManager`：基于 DashMap 的注册表
- `ToolDefinition` 定义在 `llm/provider.rs`（单一事实来源）
- 内置工具：ShellTool（Critical）、ReadTool、GrepTool、GlobTool

## Workflow / Job / Scheduler

- Workflow：轻量分组，Job 通过 depends_on 表达依赖
- Job：Standalone / Workflow / Cron 三种类型
- Scheduler：轮询调度，依赖检查，并发控制
