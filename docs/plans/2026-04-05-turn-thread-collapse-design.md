# Turn/Thread 收敛设计文档

## 背景

当前 `argus-agent` 已经明确把 `turns: Vec<TurnRecord>` 定义为线程历史的唯一真相源，但实现上仍然保留了一套较重的 `Turn` 结构：

- `Thread` 负责调度、compact、构造 turn、提交结果
- `Turn` 负责执行 LLM/tool loop，并持有一组从 `Thread` 重新打包出来的字段
- `TurnBuilder` / `TurnExecution` / `build_turn()` 形成了一条单独的包装链

这导致两个问题：

- `Turn` 和 `Thread` 之间存在明显的重复结构与重复心智模型
- 一次 turn 的真实状态来源被拆散在 `Thread` 持久态和 `Turn` 临时对象之间，不够直观

本轮目标不是改行为，而是把这条边界收紧，让 `Thread` 成为唯一状态拥有者，同时继续让 `turn.rs` 承载 turn 执行逻辑。

## 目标

本轮要实现的结果：

- `Thread` 成为 turn 运行态、生命周期和提交边界的唯一拥有者
- `turn.rs` 继续承载 turn 执行算法，但不再维护一套完整的拥有型 `Turn` 对象
- 删除 `TurnBuilder`、`TurnExecution`、`Thread::build_turn()` 这类只负责重新打包线程状态的包装层
- 尽量通过 `Vec<TurnRecord>` 动态推导 turn 编号、history、context base 和 token 相关信息
- 保持成功、失败、取消、compact 的外部行为不变

本轮不做的事：

- 不引入新的运行时概念名词来替代 `Turn`
- 不改写 `TurnRecord` 语义
- 不改变 `Checkpoint` / `TurnCheckpoint` 的持久化规则
- 不把 `turn.rs` 的执行逻辑重新塞回 `thread.rs`

## 约束

这次设计需要同时满足以下边界：

- `Vec<TurnRecord>` 仍然是唯一的历史真相源
- `turn.rs` 继续作为 turn 执行逻辑的承载文件存在
- 尽量少引入额外变量、额外结构、额外中间结果壳
- 可以牺牲 turn 独立 `spawn` 的对象能力，不再要求 turn 作为独立异步对象运行
- 取消、流式事件、tool 执行和 turn-level compact 的能力必须保留

## 方案比较

### 方案 A：保留 `Turn`，改成借用型 `Turn<'a>`

做法：

- 让 `Turn` 直接借用 `Thread`
- 把部分字段从 owned 改成 borrowed

优点：

- 表面改动较小
- 仍然保留 `Turn` 作为独立类型

缺点：

- 只是把重复结构从“拥有”改成“借用”，并没有真正消掉双层模型
- 会继续保留 `Turn` / `Thread` 两套概念边界
- 与现有 `tokio::spawn` 执行路径存在额外生命周期摩擦

### 方案 B：`Thread` 拥有状态，`turn.rs` 只保留执行逻辑

做法：

- `Thread` 直接拥有全部运行态
- `turn.rs` 改成围绕 `Thread` 的执行函数集合
- turn 成功时直接返回 `TurnRecord`

优点：

- 最符合“状态单源化”和“删除包装层”的目标
- `turn.rs` 仍然保留清晰职责，不会把执行细节全部挤回 `thread.rs`
- 可以直接删除 `TurnBuilder` / `TurnExecution` / `build_turn()` 这条链路

缺点：

- 需要重排现有 reactor 与 turn 执行的连接方式
- `turn.rs` 的部分测试入口需要从“构造 Turn”改成“调用执行函数”

### 方案 C：把 `turn.rs` 完全函数化并弱化为零散 helper

做法：

- `turn.rs` 只剩若干自由函数
- `Thread` 基本直接驱动所有执行细节

优点：

- 表面最简单

缺点：

- 容易把执行逻辑重新摊回 `thread.rs`
- turn loop 的局部可读性和可测试边界会变差

## 结论

采用方案 B。

原因：

- 它最贴近用户目标：去掉 `Turn` 和 `Thread` 之间重复的公共结构
- 它能在不发明新抽象的前提下，让 `Thread` 真正成为唯一状态拥有者
- 它保留 `turn.rs` 作为清晰的逻辑承载层，避免把 turn loop 粘回 `thread.rs`

## 设计

### 1. 状态归属

新的边界应当是：

- `Thread` 拥有线程级持久态与运行态
- `turn.rs` 只负责“这一轮怎么执行完”
- turn 成功的唯一产物仍然是 `TurnRecord`

因此以下状态都应留在 `Thread` 或继续从 `Vec<TurnRecord>` 推导：

- `turn_number`
- history / context base
- `token_count()`
- `turn_count()`
- `active_turn_cancellation`
- provider / tools / hooks / agent snapshot / senders

`turn.rs` 内部只保留真正属于本轮执行过程的局部变量，例如：

- 本轮用户输入
- 本轮 `turn_messages`
- 当前迭代里的临时 request / response / token usage
- `compacted_during_turn` 这样的瞬时控制变量

### 2. 执行边界

现有链路：

1. `Thread::begin_turn_with_number()`
2. `Thread::build_turn()`
3. `TurnBuilder`
4. `TurnExecution`
5. `Thread::finish_turn()`

目标链路：

1. `thread.rs` 的 reactor 决定开始一轮 turn
2. `Thread` 从 `turns` 动态推导 turn 编号和 context
3. `Thread` 在进入本轮前执行 thread-level compact
4. `Thread` 直接调用 `turn.rs` 中的执行入口
5. `turn.rs` 返回 `Result<TurnRecord, TurnError>`
6. `Thread` 成功时直接 append 到 `turns`

这里的关键不是发明新的 `TurnRunner`、`TurnFrame` 或 `TurnSettled`，而是把现有包装层删掉，让执行入口变成 `turn.rs` 里的私有函数或极薄 helper。

### 3. 数据流

一次 turn 的理想数据流应当是：

1. 从 `thread.turns` 推导下一个真实 `turn_number`
2. 基于最近的 `Checkpoint` / `TurnCheckpoint` 计算 turn context
3. 需要时执行 thread-level compact，并立即把 `Checkpoint(0)` 追加到 `turns`
4. 将本轮用户输入交给 `turn.rs`
5. `turn.rs` 运行 LLM -> tool -> LLM loop，并直接通过现有 sender 发出进度事件
6. 成功时返回单个 `TurnRecord`
7. `thread.rs` 把该记录提交到 `turns`，然后广播 `TurnCompleted`、`TurnSettled`、`Idle`

这里不再需要“为了能运行 turn”而专门构造一份新的 turn-owned 结构体快照。

### 4. 取消与错误处理

取消能力保留，但它只保留为真实运行时需要的原语：

- 保留 `TurnCancellation`
- 由 `Thread.active_turn_cancellation` 独占持有
- `turn.rs` 执行逻辑只读这个取消状态，不再在另一层 `Turn` 结构中复制同样的语义

错误边界收紧为：

- `turn.rs` 只返回 `Result<TurnRecord, TurnError>`
- `thread.rs` 负责把它映射为线程事件和持久化提交
- 失败和取消不落盘，不追加任何 `TurnRecord`

这次不引入新的“settled 结果对象”或“turn 结果包装壳”。

### 5. 流式事件

当前 turn 进度依赖 `TurnExecution` 句柄来传递，这条路在收敛后应被删除。

重构后：

- `turn.rs` 继续直接向现有 `thread_event_tx` / `stream_tx` 推送流式事件
- 外部观察者继续订阅线程事件，不再依赖一个独立的 turn future 句柄
- reactor 只需要关心“当前正在跑的一轮何时返回最终 `Result<TurnRecord, TurnError>`”

换句话说，保留事件能力，删除中间对象层。

## 对现有代码的影响

### 保留

- `Thread` reactor 模型
- `TurnCancellation`
- `TurnRecord`、`TurnRecordKind`
- `Checkpoint` / `TurnCheckpoint` 规则
- `turn.rs` 中的 LLM/tool/compact 主循环

### 删除或折叠

- `Turn` 作为完整拥有型执行对象的角色
- `TurnBuilder`
- `TurnExecution`
- `Thread::build_turn()`
- `begin_turn -> build_turn -> execute_progress -> finish_turn` 这条包装链中的重复状态拼装

### 保持不引入

- 不新增 `TurnFrame`
- 不新增 `TurnRunner`
- 不新增 `TurnSettled`
- 不新增任何只是为了替代旧包装层而出现的新数据壳

## 风险

### 主要风险

- reactor 和 turn 执行的耦合方式会变化，容易出现取消或 idle 事件时序回归
- 现有测试中如果大量依赖 `TurnBuilder` 直接构造 turn，迁移时会有一定机械改动
- 流式事件路径从“独立 turn 句柄”改为“thread 驱动执行”后，边界测试需要同步更新

### 风险控制

- 不改 `TurnRecord` 语义，只改拥有权和执行边界
- 先锁定行为测试，再删除包装层
- 每一步都优先删除纯转发代码，不同时引入新概念

## 验证方案

重点验证以下不变量：

1. 成功 turn 仍然只提交一个 `TurnRecord`
2. 发生 turn-level compact 时仍然提交 `TurnCheckpoint`
3. 失败或取消的 turn 仍然完全不落盘
4. `history_iter()`、`build_turn_context()`、`turn_count()`、`token_count()` 仍然与 `Vec<TurnRecord>` 一致
5. thread-level compact 仍然在开始新 turn 前执行，并按原语义写入 `Checkpoint(0)`

建议验证命令：

- `cargo test -p argus-agent`
- 如有必要补充定向测试：
  - `cargo test -p argus-agent thread::tests`
  - `cargo test -p argus-agent turn::tests`

## 实施原则

- 这次重构的目标是删除包装层，而不是重命名包装层
- 任何需要新增新概念才能解释的改动，默认不做
- 优先把可从 `Vec<TurnRecord>` 推导的状态收回到 `Thread`
- 保持 `turn.rs` 仍然是 turn 算法的归属文件，而不是让 `thread.rs` 吞掉所有逻辑
