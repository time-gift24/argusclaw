## Thread / Turn Design Standard

### Core Truth

- `Thread` 的唯一持久化真相源是 `turns: Vec<TurnRecord>`。
- 禁止引入线程级累计 token 缓存、累计 turn 编号缓存、或任何可由 `Vec<TurnRecord>` 直接推导出的镜像状态。
- `TurnRecord` 只记录成功结果：`UserTurn` 和 `Checkpoint`。
- 失败或取消的 turn 不得落成 `TurnRecord`。
- `TurnRecord.token_usage` 只表达本轮 turn 或 compact 调用返回的 usage，同时也是 compact 判断依据。

### Turn Boundary

- 成功的 `Turn` 唯一结果类型是 `TurnRecord`；禁止再引入 `TurnOutput` 这类中间结果壳。
- `Turn` 负责把一次成功执行直接收敛成最终 `TurnRecord`，包括 transcript 归一化、时间戳和 usage。
- `Thread` 只负责调度、取消、追加成功返回的 `TurnRecord`，不负责二次修补 transcript。
- `Turn` 不直接持有 `Thread`；线程上下文应以快照值传入 `Turn`，避免运行中回读线程可变状态。

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

### Thread Metadata

- `thread.json` 是 thread 节点元数据，不再是裸 `AgentRecord`。
- `thread.json` 至少要包含：
  - `thread_id`
  - `kind`
  - `root_session_id`
  - `parent_thread_id`
  - `child_thread_ids`
  - `agent_snapshot`
- thread 级 agent 配置、job 父子关系、以及 trace 树结构都以 `thread.json` 为 authority。

### Job Tree Rules

- chat thread 一定是树根，`parent_thread_id = None`。
- job thread 一定是其派发源 thread 的直接子节点，并持久化到父节点目录下。
- `child_thread_ids` 只记录直接子节点，不记录整棵子树。
- `ThreadPool` 中的 `parent_thread_by_child` / `child_threads_by_parent` 只是运行时缓存，不是真相源；恢复时必须从 `thread.json` 回填。
- job thread 一旦创建，父节点固定；发现持久化父节点与当前派发源不一致时应直接报错，不做隐式迁移。

### Simplicity Guardrails

- 优先删除冗余包装结构，而不是叠加新状态层。
- 任何新增状态都必须回答：它是否不能从 `turns: Vec<TurnRecord>`、`thread.json` 或运行时 mailbox 直接推导。
- 若答案是否定的，该状态应删除，而不是缓存。
