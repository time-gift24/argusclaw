# Argus-Agent

> 特性：线程拥有的 turn 运行时，负责计划、compact、trace 与已结算 turn log。

## 核心职责

- `Thread` 是对外公开入口；`Turn` 仍是内部执行细节
- 已结算历史的事实来源是 `turns: Vec<TurnRecord>` 与 trace 目录中的 `turns.jsonl`
- thread / turn 级 compact、plan store、hook、tool context 都在这里汇聚

## 关键模块

- `src/thread.rs`：`Thread` actor、顺序执行 turn、广播 `ThreadEvent`
- `src/thread_runtime.rs`：thread runtime 注册、订阅与运行时关系缓存
- `src/turn.rs`：LLM / tool 执行循环、stream 归并、hook 调度
- `src/history.rs`：`TurnRecord`、`TurnRecordKind`、turn 编号推导
- `src/compact/*`：thread-level / turn-level compact
- `src/plan_*`：plan 持久化与 `update_plan` tool
- `src/thread_trace_store.rs`、`src/turn_log_store.rs`、`src/trace.rs`：trace 与恢复

## 公开入口

- `Thread`、`ThreadBuilder`
- `ThreadRuntime`
- `ThreadConfig`、`TurnConfig`
- `LlmThreadCompactor`、`LlmTurnCompactor`
- `FilePlanStore`

## 修改守则

- 不要引入能从 `TurnRecord` 或 trace 文件直接推导出来的镜像状态
- `Checkpoint(0)` 只表示线程级 compact；真实 turn 编号由 `UserTurn` / `TurnCheckpoint` 消耗
- trace 节点文件固定为 `thread.json`、`turns.jsonl`、`plan.json`
- parent/child thread 关系的持久化真相源仍是目录树 + `thread.json`；运行时 map 只允许作为缓存
- 更细的不变量见同目录 `AGENTS.md`
