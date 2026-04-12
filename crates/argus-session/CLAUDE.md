# Argus-Session

> 特性：session 聚合层，负责 thread 装载、恢复、scheduler backend 与 job / mailbox 协调。

## 核心职责

- `SessionManager` 创建、加载、重命名、删除 session 与 thread
- chat thread 的 runtime 生命周期优先通过 `argus-agent::ThreadRuntime` 管理
- 从 `argus-agent` 的 trace / turn log 恢复 thread 状态
- 为 `scheduler` tool 提供 backend，把 template、job、mailbox 组合成会话层能力，并从当前 agent 的 `subagent_names` 解析可调度子代理
- 持有内存态 `Session` 缓存，并把事件广播给上层

## 关键模块

- `src/manager.rs`：`SessionManager`、恢复逻辑、scheduler backend
- `src/session.rs`：`Session`、`SessionSummary`、`ThreadSummary`
- `src/provider_resolver.rs`：对 `argus-protocol::ProviderResolver` 的 re-export

## 公开入口

- `SessionManager`
- `Session`
- `SessionSummary`
- `ThreadSummary`

## 修改守则

- session 是 orchestration layer，不要把 provider/tool 实现细节塞进这里
- chat thread ownership 在 session 侧要尽量经由 `ThreadRuntime`，不要重新把这类状态收回 job runtime supervisor
- 恢复逻辑必须与 `argus-agent` 的 trace / turn 语义保持一致
- `scheduler`、mailbox 或 inbox 语义变更时，要同步检查 `argus-tool` 协议与桌面端消费者
- 调度递归保护属于 runtime 责任，使用 dispatch depth guard，不要把层级约束重新落回持久化模型
