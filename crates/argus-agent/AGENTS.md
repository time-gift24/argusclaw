先熟悉同目录 `CLAUDE.md`。

以下内容是在 `CLAUDE.md` 之上的补充不变量，主要约束 thread / turn / trace 的事实来源与结算语义。

## Thread / Turn Design Standard

### Core Truth

- `Thread` 的唯一持久化真相源是 `turns: Vec<TurnRecord>`。
- 禁止引入线程级累计 token 缓存、累计 turn 编号缓存、或任何可由 `Vec<TurnRecord>` 直接推导出的镜像状态。
- `TurnRecord` 只记录成功结果：`UserTurn`、`Checkpoint` 和 `TurnCheckpoint`。
- 失败或取消的 turn 不得落成 `TurnRecord`。
- `TurnRecord.token_usage` 只表达本轮 turn 或 compact 调用返回的 usage，同时也是 compact 判断依据。
- `Checkpoint` 是线程级上下文快照，`turn_number = 0`。
- `TurnCheckpoint` 是“发生过 turn 内 compact 的成功 turn”，必须保留真实 `turn_number`，并参与编号、恢复和 transcript 读取。

### Turn Boundary

- 成功的 `Turn` 唯一结果类型是 `TurnRecord`；禁止再引入 `TurnOutput` 这类中间结果壳。
- `Turn` 负责把一次成功执行直接收敛成最终 `TurnRecord`，包括 transcript 归一化、时间戳和 usage。
- `Thread` 只负责调度、取消、追加成功返回的 `TurnRecord`，不负责二次修补 transcript。
- `Turn` 不直接持有 `Thread`；线程上下文应以快照值传入 `Turn`，避免运行中回读线程可变状态。
- 未发生 turn 内 compact 的成功 turn 结算为 `UserTurn`。
- 发生过 turn 内 compact 的成功 turn 结算为单个 `TurnCheckpoint`；禁止再为同一 turn 持久化中间 compact 记录数组。

### Compaction Rules

- `Compactor` / `CompactResult` 是共享抽象；thread 和 turn 只是两个不同实现，不再拆双 trait。
- compact 是否触发、threshold 如何计算，都必须由具体 `Compactor` 实现内部决定；调用方只传入当前完整消息和 token 估值。
- thread-level compact 在开始新 turn 前运行；成功时立即追加 `Checkpoint(0)`。
- turn-level compact 在 `Turn::execute_loop` 中对“当前真实 request messages”运行，禁止基于陈旧 history 快照判断。
- turn-level compact 成功后只更新运行中的 `self.history` / `turn_messages`；若该 turn 最终失败或取消，不得留下任何持久化记录。
- turn-level compact 一旦发生且该 turn 最终成功，最终返回值必须是单个 `TurnCheckpoint`，而不是 `Checkpoint... + UserTurn` 或其他包装结构。

### Prompt / Agent Snapshot

- `system_prompt` 不是持久化 transcript 的一部分，禁止写入任何 `TurnRecord.messages`。
- 每个 thread 在创建时冻结一份 `AgentRecord` snapshot，后续所有 turn 请求都使用这份 snapshot 的 prompt/tools/描述。
- 已存在 thread 的运行时行为不再跟随最新模板漂移；恢复时只能从 thread 自己的 snapshot 读取 agent 配置。
- prompt 只在发起 LLM request 时临时 prepend 到上下文，不进入持久化消息历史。

### Trace Node Layout

- 每个 thread 的 trace 必须收敛到单一节点目录，不再拆出 `turns/meta.jsonl` 之类的额外层级。
- 标准节点文件固定为：
  - `thread.json`
  - `turns.jsonl`
  - `plan.json`
- chat root 路径：`{trace_root}/{session_id}/{thread_id}/`
- job child 路径：`{parent_thread_dir}/{child_thread_id}/`
- `TraceConfig` 必须持有显式 `thread_base_dir`；禁止再由 `trace_root + session_id + thread_id` 在运行时反推目录。

### Turn Log / Recovery Rules

- `turns.jsonl` 是 append-only 的 turn 真相源；恢复逻辑必须以它为 authority，而不是额外计数状态。
- `recover_thread_log_state()` 允许若干前置 `Checkpoint(0)`，但第一条“已结算 turn”必须是 `UserTurn(1)` 或 `TurnCheckpoint(1)`。
- `derive_next_user_turn_number()`、`turn_count()` 和恢复时的单调递增校验，都必须把 `UserTurn` 与 `TurnCheckpoint` 视为已消耗的真实 turn。
- `history_iter()`、`flatten_turn_messages()`、`RecoveredThreadLogState::committed_messages()` 必须包含 `UserTurn` 与 `TurnCheckpoint`，仅跳过 `Checkpoint(0)`。
- `build_turn_context()` 必须把最近的 `Checkpoint` 或 `TurnCheckpoint` 视为上下文基底；其后只追加后续 `UserTurn` transcript。
- transcript 读取、session snapshot、job summary / memory estimation 等读取侧，禁止默默退回到“只看 UserTurn”的旧语义。

### Thread Metadata

- `thread.json` 是 thread 节点元数据，不再是裸 `AgentRecord`。
- `thread.json` 至少要包含：
  - `thread_id`
  - `kind`
  - `root_session_id`
  - `parent_thread_id`
  - `job_id`
  - `agent_snapshot`
- thread 级 agent 配置以 `thread.json` 为 authority。
- job 父子关系以“树形目录 + 子节点 `thread.json.parent_thread_id` / `job_id`”为 authority。

### Job Tree Rules

- chat thread 一定是树根，`parent_thread_id = None`。
- job thread 一定是其派发源 thread 的直接子节点，并持久化到父节点目录下。
- job thread 必须持久化自己的 `parent_thread_id` 和 `job_id`；父节点不再维护持久化 child 列表。
- runtime registry / job runtime supervisor 中的 `parent_thread_by_child` / `child_threads_by_parent` 只是运行时缓存，不是真相源；恢复时必须从目录树和子节点元数据回填。
- job thread 一旦创建，父节点固定；发现持久化父节点与当前派发源不一致时应直接报错，不做隐式迁移。

### Simplicity Guardrails

- 优先删除冗余包装结构，而不是叠加新状态层。
- 任何新增状态都必须回答：它是否不能从 `turns: Vec<TurnRecord>`、`thread.json` 或运行时 mailbox 直接推导。
- 若答案是否定的，该状态应删除，而不是缓存。
