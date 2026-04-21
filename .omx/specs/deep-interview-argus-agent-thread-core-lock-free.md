# Deep Interview Spec: argus-agent Thread Core Lock-Free

## Metadata

- Date: 2026-04-20T15:16:56Z
- Profile: standard
- Rounds: 7
- Final ambiguity: 0.197
- Threshold: 0.20
- Context type: brownfield
- Context snapshot: `.omx/context/argus-agent-thread-core-lock-free-20260420T150100Z.md`
- Transcript: `.omx/interviews/argus-agent-thread-core-lock-free-20260420T151656Z.md`

## Clarity Breakdown

| Dimension | Score |
| --- | --- |
| Intent | 0.82 |
| Outcome | 0.79 |
| Scope | 0.82 |
| Constraints | 0.90 |
| Success Criteria | 0.44 |
| Context | 0.84 |

Brownfield ambiguity formula:

`1 - (intent*0.25 + outcome*0.20 + scope*0.20 + constraints*0.15 + success*0.10 + context*0.10) = 0.197`

## Intent

用户要纠正的不是某个已知死锁 bug，而是更根本的模型表达错误：

- `Mutex` / `RwLock` 让 `Thread` 看起来像一个共享可变对象
- 这掩盖了 `Thread` 本应具备的单拥有者 runtime 角色
- 也让实现和测试都围绕共享锁展开，复杂化了设计叙事

用户想把 `Thread` 明确为 `argus-agent` 的核心 runtime owner，并用这个 owner 模型反推 API、测试和跨 crate 交互方式。

## Desired Outcome

确定以下终态作为后续规划/实现的源事实：

1. `Thread` 在 `argus-agent` 中被强化为唯一 runtime 核心与事实入口
2. `Thread` runtime 主路径不再依赖 `Mutex` / `RwLock` 这一类共享锁
3. 跨 crate 主路径不再以 `Arc<RwLock<Thread>>` 暴露或传递 thread
4. 运行时写权限收敛到唯一 owner
5. 观察者只通过 mailbox / event / snapshot / query-handle 一类接口交互
6. 读取不追求强一致，只追求最终一致性，读取信息可直接返回
7. Thread 相关测试也不再使用共享锁模型来表达主路径
8. `mpsc` / `broadcast` / `atomic` 这类单写者友好协调原语可以保留

## In Scope

- 重新定义 `Thread` 的 owner/observer 边界
- 去掉 `argus-agent` 中 `Thread` runtime 主路径对 `Mutex` / `RwLock` 的依赖
- 去掉以下 crate 主路径中的 `Arc<RwLock<Thread>>` 传递模型：
  - `argus-agent`
  - `argus-session`
  - `argus-thread-pool`
  - `argus-job`
- 让主路径测试也跟随新的 owner 模型收口
- 允许为此调整 handle 形态、读侧 API 形态和迁移兼容层

## Out of Scope / Non-goals

- 不追求消灭所有同步原语
- 不把 `mpsc` / `broadcast` / `atomic` 当成此次无锁化的清除对象
- 不以“绝对零锁”为目标去重写 channel、broadcast、repository、trace 持久化等机制
- 不把 `plan_store`、repository、trace 持久化重写当成本轮核心目标，除非这些改动是为了配合去掉 `Arc<RwLock<Thread>>` 所必需
- 不接受“生产无锁、测试继续拿共享锁模拟”的双轨表达；测试例外不是 non-goal

## Decision Boundaries

以下事项 OMX 在后续 `ralplan` / 执行阶段可自行决定，无需再次确认：

- `ThreadHandle` / `ThreadRuntime` / `ThreadOwner` 等具体命名
- 迁移顺序与兼容层策略
- 读侧 API 采用 snapshot、cached fields 还是轻量 query handle
- `argus-session` / `argus-thread-pool` / `argus-job` 各自适配新 thread handle 的具体做法
- `plan_store`、repository、trace 持久化在为去掉 `Arc<RwLock<Thread>>` 服务时的必要配套调整

以下事项已被拍板，不应在后续规划中被悄悄改写：

- `Thread` runtime 主路径必须基本不再依赖 `Mutex` / `RwLock`
- 跨 crate 主路径必须去掉 `Arc<RwLock<Thread>>`
- 相关测试也必须去掉共享锁表达
- 读侧不追求强一致，只追求最终一致性
- `mpsc` / `broadcast` / `atomic` 可以保留

## Constraints

- 必须突出 `Thread` 作为 runtime owner 的核心地位
- 必须维持单拥有者写权限模型
- 观察者不需要一致性读保障
- 允许直接返回读取结果，只要求最终一致性
- 无锁化的目标是移除共享锁模型，而不是移除所有协调机制
- 需要遵守已有 brownfield 边界：
  - `argus-agent` 仍是 thread-owned runtime authority
  - `argus-thread-pool` 仍是 runtime residency / lifecycle 基础设施，而不是上层语义层

## Testable Acceptance Criteria

1. `Arc<RwLock<Thread>>` 从 `argus-agent`、`argus-session`、`argus-thread-pool`、`argus-job` 的主路径中消失
2. `Thread` runtime 的可变状态推进收敛到明确的单 owner 路径，而不是通过共享锁在多处读写
3. 外部调用方不再通过共享锁访问或操纵 `Thread`
4. 读侧 API 明确体现“直接返回 + 最终一致性”，而不是暗示强一致读
5. Thread 主路径相关测试不再使用 `Mutex` / `RwLock` 来包装或驱动 `Thread`
6. 允许保留 `mpsc` / `broadcast` / `atomic` 等协调原语，且这些原语不被视为违反目标
7. `plan_store`、repository、trace 持久化不会被作为独立重写目标，除非它们是去除 `Arc<RwLock<Thread>>` 的必要配套

## Assumptions Exposed And Resolutions

| Exposed assumption | Resolution |
| --- | --- |
| “无锁化”也许只是把锁藏到 API 后面 | 否。用户要的是 runtime 本体也基本不依赖共享锁 |
| 测试可以继续保留旧锁模型，只要生产代码变了就行 | 否。Thread 主路径相关测试也要去掉共享锁表达 |
| 用户可能反对所有同步原语 | 否。`mpsc` / `broadcast` / `atomic` 明确保留 |
| 这轮也许只改 `argus-agent` 内部就够了 | 否。跨 crate 主路径上的 `Arc<RwLock<Thread>>` 也要一起拿掉 |

## Pressure-pass Findings

被 revisited 的核心答案是：

- 初始回答只是“锁让模型更复杂”，并且承认主要是凭感觉提出

压力测试问题把它推进成了更清晰的硬边界：

- 是否只隐藏 `Arc<RwLock<Thread>>`，还是 runtime 本体也应基本无锁？
- 是否允许测试继续用共享锁表达旧模型？
- 是否连 `mpsc` / `broadcast` / `atomic` 一并排斥？

结果变化：

- 从“感觉上可以无锁”收口为“必须去掉共享锁模型”
- 从“可能只是接口去锁”收口为“runtime 本体也基本不依赖共享锁”
- 从“可能只改生产路径”收口为“测试也必须跟进”
- 从“可能排斥所有同步”收口为“保留单写者友好协调原语”

## Brownfield Evidence Vs Inference

### Evidence

- `crates/argus-agent/src/thread.rs` 当前仍以 `Thread::spawn_reactor(thread: Arc<RwLock<Self>>)` 启动 runtime
- 同文件内 runtime 状态推进依赖 `thread.read().await` / `thread.write().await`
- `crates/argus-session/src/session.rs` 使用 `Weak<RwLock<Thread>>`
- `crates/argus-thread-pool/src/lib.rs` 多处保存并传递 `Arc<RwLock<Thread>>`
- `crates/argus-job/src/thread_pool.rs` 也沿用了相同形态
- 上一份 spec `.omx/specs/deep-interview-session-threadpool-job-boundaries.md` 已明确 `argus-agent` 是 thread-owned runtime crate

### Inference

- 现状已经形成“reactor 实际单写，但 API 形态仍是假共享对象”的错位
- 读锁的大量存在更像是为了触达 handle，而不是为了保护真正需要多写者并发的模型
- 因此一个 owner/observer 分离的 thread handle 体系，更符合当前实际执行语义和用户目标

## Technical Context Findings

- `Thread` 当前同时承担：
  - runtime state
  - queued messages
  - active turn cancellation
  - committed turns
  - event broadcast
- 这些状态现在被 `Arc<RwLock<Thread>>` 包裹后，外界与内部都通过共享锁访问
- 但从行为看，真正推进 runtime 的仍是 thread reactor，本质更接近单 owner task
- 因此后续规划重点应是：
  - 让 owner 明确化
  - 让 handle 外露化
  - 让读侧语义从“共享对象读锁”转成“最终一致 snapshot / query”

## Residual Risk

- 成功标准中的具体 API 形态仍有设计空间
- “读取信息就直接返回即可”被转写成了“最终一致 snapshot/query 语义”，这在后续规划阶段应保持不被重新解释成强一致读

## Recommended Handoff

推荐下一步进入：

`$ralplan .omx/specs/deep-interview-argus-agent-thread-core-lock-free.md`

推荐原因：

- 当前已经不再缺需求边界，而是缺具体迁移方案
- 这次改动跨 `argus-agent` / `argus-session` / `argus-thread-pool` / `argus-job`，需要先把 owner-handle 拆分、兼容层、迁移顺序和测试策略设计清楚
- 这一步比直接进实现更稳，也更符合你已经给出的自主决策边界
