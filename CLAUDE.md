# ArgusClaw 开发指南

## 构建与测试

```bash
cargo fmt                                                    # 格式化
cargo clippy --all --benches --tests --examples --all-features  # 检查 (零警告)
cargo test                                                   # 单元测试
cargo test --features integration                            # + Sqlite 测试
RUST_LOG=argusclaw=debug,claw=debug cargo run  # 开启日志运行
```

## 设计原则(非常重要)
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

```text
crates/
├── claw/
│   ├── src/
│   │   ├── lib.rs                    # 库入口，模块声明和导出
│   │   ├── error.rs                  # 顶层错误类型
│   │   ├── claw.rs                   # AppContext；拥有 LLMManager、AgentManager
│   │   ├── agents/                   # Agent 管理
│   │   │   ├── mod.rs                # AgentManager (占位)
│   │   │   └── turn/                 # Turn 执行模块
│   │   │       ├── mod.rs            # Turn 模块入口和重导出
│   │   │       ├── config.rs         # TurnConfig, TurnInput, TurnOutput, TokenUsage
│   │   │       ├── error.rs          # TurnError 类型
│   │   │       ├── hooks.rs          # Hook 系统 (HookEvent, HookHandler, HookRegistry)
│   │   │       └── execution.rs      # execute_turn 及并行工具支持
│   │   ├── db/                       # 存储抽象和实现
│   │   │   ├── mod.rs                # DB 模块入口和共享错误
│   │   │   ├── llm.rs                # LLM 提供商记录和仓库 trait
│   │   │   └── sqlite/               # SQLx 支持的 SQLite 实现
│   │   │       ├── mod.rs            # SQLite 连接和迁移辅助
│   │   │       └── llm.rs            # SQLite LLM 提供商仓库
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
│   │   └── tool/                     # Agent/LLM 工具注册表
│   │       └── mod.rs                # NamedTool trait、ToolManager、ToolError
│   ├── migrations/                   # SQLx 迁移
│   └── tests/                        # E2E 测试；不适合内联测试的多模块场景
│       └── turn_integration_test.rs  # Turn 模块集成测试
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
