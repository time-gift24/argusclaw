# Context Snapshot: argus-agent thread core lock-free

## Task statement

用户希望在 `argus-agent` 中进一步突出 `Thread` 的核心地位，并推动“无锁化”设计方向。

## Desired outcome

澄清一份可执行规格，明确：

- `Thread` 是否应成为唯一写拥有者
- 观察者/订阅者应保留哪些只读能力
- 现有 `Arc<RwLock<Thread>>` 模型应被收敛到什么形态
- 本轮是聚焦 `argus-agent` 内部 Thread runtime，还是要连带影响 `argus-thread-pool` / `argus-session` 的对外 API

## Stated solution

用户提出的方向是：

- 太多锁，想做无锁化设计
- 只有唯一持有者才有写权限
- 其他观察者不需要读一致性保证
- 系统只追求最终一致性

## Probable intent hypothesis

用户不是单纯追求“去掉几个 `RwLock`”，而是想把 runtime ownership 模型重新表达清楚：

- `Thread` 应更像单写者 actor / router，而不是共享可变对象
- 外部系统应该通过 mailbox / event / snapshot 与 thread 交互，而不是拿共享锁直接读写内部状态
- 锁的存在正在掩盖职责边界，使 `Thread` 的核心地位不够显性

## Known facts / evidence

- `crates/argus-agent/AGENTS.md` 已明确：`Thread` 是公开入口，thread/turn 的事实来源集中在 `argus-agent`
- 当前 `crates/argus-agent/src/thread.rs` 公开了 `Thread::send_message(&self, ...)` 和 `Thread::spawn_reactor(thread: Arc<RwLock<Thread>>)`
- 当前 reactor 生命周期依赖 `Arc<RwLock<Thread>>` 来：
  - 取走 `message_rx`
  - 读 `runtime_state`
  - 改 `pending_messages`
  - 设定/清理 `active_turn_cancellation`
  - 结算 turn 后写回 `turns`
- `crates/argus-session/src/session.rs`、`crates/argus-thread-pool/src/lib.rs`、`crates/argus-job/src/thread_pool.rs` 目前都以 `Arc<RwLock<Thread>>` 持有或传递 thread
- 现有上一轮 deep-interview 规格 `.omx/specs/deep-interview-session-threadpool-job-boundaries.md` 已明确：
  - `argus-agent` 是 thread-owned runtime crate
  - `argus-thread-pool` 只负责 runtime residency / lifecycle，不应承载更高层语义

## Constraints

- 本轮仍是 requirements / design clarification，不直接进入实现
- 需要尊重现有仓库边界：`argus-agent` 管 thread-owned runtime 真相源
- 需要弄清“无锁化”是否是强约束，还是“减少共享锁暴露、保留少量内部同步”即可
- 需要明确是否允许改变跨 crate 的 public API

## Unknowns / open questions

- 用户要优化的首要问题是：概念表达、并发语义、API 手感，还是性能/死锁风险
- “无锁化”是指彻底消灭 `RwLock<Thread>`，还是只要求不再暴露共享写模型
- 观察者允许读取什么：即时快照、近似统计、事件流、持久化 trace，还是全部都可接受
- `ThreadPool` / `Session` 是否仍可持有 thread handle，如果可以，handle 形态应该是什么

## Decision-boundary unknowns

- OMX 后续是否可自行决定 `ThreadHandle` / `ThreadActor` / `ThreadSnapshot` 等具体命名
- OMX 后续是否可自行决定渐进迁移策略和兼容层
- OMX 后续是否可自行决定哪些读 API 改成 snapshot/event 驱动
- 仍未明确是否可修改 `argus-thread-pool` 与 `argus-session` 的对外类型签名

## Likely codebase touchpoints

- `crates/argus-agent/src/thread.rs`
- `crates/argus-agent/src/thread_bootstrap.rs`
- `crates/argus-session/src/session.rs`
- `crates/argus-session/src/manager.rs`
- `crates/argus-thread-pool/src/lib.rs`
- `crates/argus-job/src/thread_pool.rs`
