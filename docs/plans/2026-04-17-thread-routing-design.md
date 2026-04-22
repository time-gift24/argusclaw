# Thread 路由化重构设计

**日期：** 2026-04-17

## 目标

把 `Thread` 收敛成唯一的消息路由入口，删除 `ThreadMailbox` 及其衍生语义。

重构后的线程模型只保留一个外部写入口：

```rust
thread.send_message(ThreadMessage)
```

`Thread` 负责接收、路由、顺序消费消息；外部不再持有任何 mailbox / inbox / endpoint / bus 句柄。

## 当前问题

现有 `ThreadMailbox` 同时承担了过多职责：

- 运行时输入队列
- mailbox message 队列
- stop signal
- unread / mark read
- `claim_job_result(job_id)` 去重补丁

这导致几个结构性问题：

1. `ThreadMailbox` 既像 runtime ingress，又像 inbox store，语义混杂。
2. `Session`、`ThreadPool`、`Thread` 共享同一个可变对象，控制面和数据面耦合。
3. 外部通过 mailbox 直接改线程内部状态，`Thread` 不是唯一事实来源。
4. `claim_job_result(job_id)` 说明 job result 同时存在于 job store 和 mailbox 队列，模型重复。

问题的核心不是锁重，而是模型错位。

## 设计决策

### 1. 删除 `ThreadMailbox`

以下类型整体删除：

- `ThreadMailbox`
- `ThreadMailboxItem`
- 所有对 `Arc<Mutex<ThreadMailbox>>` 的公开暴露

线程内部如果仍然需要排队，队列只能作为 `Thread` 的私有状态存在，不再以共享对象形式暴露。

### 2. `ThreadMessage` 成为统一消息信封

`ThreadMailboxItem` 直接升级成统一消息枚举：

```rust
pub enum ThreadMessage {
    UserInput {
        content: String,
        msg_override: Option<MessageOverride>,
    },
    PeerMessage {
        message: MailboxMessage,
    },
    JobResult {
        message: MailboxMessage,
    },
    Interrupt,
    Control(ThreadControlMessage),
}
```

`ThreadControlMessage` 只保留线程内部确实需要的控制项，例如 `ShutdownRuntime`。

### 3. `Thread::send_message` 是唯一外部入口

外部世界不再调用：

- `enqueue_user_message`
- `enqueue_mailbox_message`
- `interrupt_thread` 对 mailbox 的直接修改
- `claim_job_result`
- 任何 mailbox 读写接口

统一改成：

```rust
thread.send_message(ThreadMessage::...)
```

这条规则适用于：

- `SessionManager`
- `Session`
- `ThreadPool`
- `JobManager`
- scheduler 派生出来的跨 thread 通信

### 4. `Thread` 自己做路由，不引入中间层

不引入 `ThreadEndpoint`、`RuntimeSubscriber`、`ProjectionSubscribers` 等额外概念。

`Thread` 自己内部 `match ThreadMessage` 做路由：

- `UserInput` / `PeerMessage` / `JobResult` 进入顺序执行路径
- `Interrupt` 进入高优先级控制路径
- `Control` 进入内部控制路径

### 5. 去掉 inbox 语义

这次重构只保留纯 `send_message` 路由能力。

以下语义从 `Thread` 层删除：

- `unread_mailbox_messages()`
- `mark_mailbox_message_read()`
- `check_inbox`
- `mark_read`

这意味着 `Thread` 不再承担 inbox 产品能力，只保留消息路由和 turn 调度能力。

### 6. `JobManager` 是 job result 唯一真相源

`claim_job_result(job_id)` 从 `Thread` 层删除。

原因：

- `claim_job_result` 的存在只是为了从 mailbox 队列中间摘掉一条 `job result`，避免和 job store 重复消费。
- 删除 `ThreadMailbox` 后，`Thread` 不再保存可供 claim 的共享 job-result 队列。
- job 结果的 lookup / consume / persisted state 全部回归 `JobManager`。

`ThreadMessage::JobResult` 只是通知消息，不再承载 claim/consume 语义。

## 重构后的线程模型

### 外部工作方式

外部只做两件事：

1. 找到目标 `Thread`
2. 调用 `send_message(ThreadMessage)`

示例：

- 用户发消息  
  `SessionManager -> thread.send_message(ThreadMessage::UserInput { ... })`
- 一个 thread 给另一个 thread 发普通消息  
  `scheduler/session -> thread.send_message(ThreadMessage::PeerMessage { ... })`
- job 完成回传  
  `JobManager/SessionManager -> thread.send_message(ThreadMessage::JobResult { ... })`
- 中断当前 turn  
  `Session -> thread.send_message(ThreadMessage::Interrupt)`

外部不再：

- 读取 mailbox
- 持有 mailbox 句柄
- 改 mailbox 队列
- 用 mailbox 唤醒 runtime

### `Thread` 内部状态

`Thread` 内部保留两个关键状态：

- 消息接收通道
- 私有 `pending_messages: VecDeque<ThreadMessage>`

注意：删除 `ThreadMailbox` 不等于线程内部绝对没有队列；含义是“不再有共享 mailbox 对象”。如果线程在运行中收到新的可执行消息，这些消息进入 `Thread` 私有队列，由线程自己在 turn settle 后继续消费。

### 处理规则

`Thread` 接到消息后的规则如下：

- `UserInput`
  - 若线程空闲：立即启动下一轮 turn
  - 若线程忙碌：追加到 `pending_messages`
- `PeerMessage`
  - 与 `UserInput` 一样进入顺序消费路径
- `JobResult`
  - 与 `PeerMessage` 一样进入顺序消费路径
  - 不再支持从 `Thread` 内部 claim / retract
- `Interrupt`
  - 若当前有 active turn：立即触发取消
  - 不进入普通 FIFO 队列
  - 不影响后续排队消息
- `Control(ShutdownRuntime)`
  - 空闲时直接关闭
  - 运行中先取消当前 turn，settle 后退出

## 保留的不变量

### FIFO

`UserInput` / `PeerMessage` / `JobResult` 仍保持全局 FIFO。

如果线程忙碌，它们进入 `Thread` 私有 `pending_messages`，按收到顺序依次启动后续 turn。

### Interrupt 只影响当前 turn

`Interrupt` 不进入普通消息队列，因此不会误伤下一个 turn。

### `Thread` 是唯一运行时事实来源

所有会改变线程执行状态的消息都必须经过 `Thread::send_message`。

外部系统不能直接修改线程内部排队状态。

## 公开接口变化

### 新接口

- `Thread::send_message(ThreadMessage)`

### 删除接口

- `Thread::mailbox()`
- `Session::mailbox()`
- `Session::enqueue_user_message()`
- `Session::enqueue_mailbox_message()`
- `Session::claim_job_result()`
- `ThreadPool::unread_mailbox_messages()`
- `ThreadPool::mark_mailbox_message_read()`
- scheduler backend 的 `check_inbox` / `mark_read`

## 迁移范围

本次重构至少影响以下文件：

- `crates/argus-protocol/src/events.rs`
- `crates/argus-agent/src/thread.rs`
- `crates/argus-session/src/session.rs`
- `crates/argus-session/src/manager.rs`
- `crates/argus-job/src/thread_pool.rs`
- `crates/argus-tool/src/scheduler.rs`

测试也需要同步更新：

- `argus-protocol` 中的 mailbox 单测改写为 `ThreadMessage` / 路由语义测试
- `argus-agent` 中的 runtime 队列与 interrupt 语义测试改写为 `send_message` 模型
- `argus-session` / `argus-job` 中所有 mailbox/inbox 测试改为 thread routing 测试

## 非目标

这次重构不做以下事情：

- 不保留 inbox 产品能力
- 不设计独立 bus 框架
- 不引入 endpoint / subscriber 抽象层
- 不让 `Thread` 感知 `JobManager` 的 claim / consume 细节

## 风险

1. scheduler `check_inbox` / `mark_read` 删除属于公开能力收缩，需要同步消费者。
2. `Interrupt` 从 mailbox flag 改成消息路由后，必须重新验证“只取消当前 turn”的语义。
3. `Thread` 内部私有队列取代共享 mailbox 后，eviction / reload 语义要重新检查，避免遗漏 pending message。

## 验证重点

实现时必须重点验证：

- 连续发送多条 `UserInput` 时 FIFO 不变
- `PeerMessage` 与 `JobResult` 混排时 FIFO 不变
- `Interrupt` 不会排进下一轮 turn
- runtime busy 时新消息会进入 `Thread` 私有队列
- `ShutdownRuntime` 在 idle/running 两种状态下都能正确结束
- 删除 inbox / claim 接口后，scheduler / session / job 层编译与测试全部收口
