# Argus-Repository

> 特性：Repository traits + SQLite 实现，统一承载 agent、session、thread、job、account、mcp、provider 持久化。

## 作用域

- 本文件适用于 `crates/argus-repository/` 及其子目录。

## 核心职责

- `traits/*` 定义聚合边界上的 repository contract
- `sqlite/*` 由 `ArgusSqlite` 统一实现，并复用同一 `SqlitePool`
- `types/*` 放持久化 record / ID / DTO
- 负责连接、迁移与加密 secret 的读写辅助
- 维护扁平化 agent schema，包括 `subagent_names` 的 JSON 持久化与迁移

## 关键模块

- `src/traits/*`
- `src/sqlite/mod.rs`
- `src/sqlite/account.rs`、`agent.rs`、`job.rs`、`llm_provider.rs`、`mcp.rs`、`session.rs`、`thread.rs`
- `src/types/*`
- `src/error.rs`

## 公开入口

- `ArgusSqlite`
- `connect`、`connect_path`、`migrate`
- 所有 repository traits
- `AgentRunRepository` 负责外部 agent run registry 的持久化状态，不与 chat `ThreadId` 混作公开 ID

## 修改守则

- 这里是仓库里唯一允许写 SQL 的地方
- secret 字段必须走 cipher 读写路径，兼顾 fallback decrypt 场景
- 涉及多个聚合的一致性更新时，优先放在 `ArgusSqlite` 的事务能力内完成
- agent 模板关系只持久化 `subagent_names`，不要重新引入 `parent_agent_id` 或类型区分
