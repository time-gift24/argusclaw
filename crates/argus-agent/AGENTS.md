## TurnRecord Constraints

- `Thread` 的唯一持久化真相源是 `turns: Vec<TurnRecord>`，禁止存储线程级累计 token 状态。
- `TurnRecord` 只记录成功结果：`UserTurn` 和 `Checkpoint`。
- `TurnRecord.token_usage` 只表达本轮 turn 或 compact 调用返回的 usage，同时也是 compact 判断依据。
