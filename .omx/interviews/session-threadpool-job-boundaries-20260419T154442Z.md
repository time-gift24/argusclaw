# Deep Interview Transcript Summary

- Date: 2026-04-19T15:44:42Z
- Profile: standard
- Context type: brownfield
- Final ambiguity: 0.11
- Threshold: 0.20
- Context snapshot: `.omx/context/session-threadpool-job-boundaries-20260419T031842Z.md`

## Condensed outcome

本轮没有进入实现，目标是把 `Session`、`ThreadPool`、`Job` 的职责边界和 crate 归属定清楚。

最终结论：

1. crate 归属本轮必须一起定。
2. 不把 `ThreadPool` 并入 `argus-agent`。
3. 新增独立 crate：`argus-thread-pool`。
4. `ThreadPool` 保留原名，不做 rename pass。
5. 新 crate 选择最小职责模型：
   `ThreadPool` 是纯 runtime pool 基础设施，只负责 `Thread` 的用量管理与监控。
6. `ThreadPool` 禁止知道 `Session`、`Job` 语义。
7. `SessionManager`、`JobManager` 都要轻量化：
   它们只从 `ThreadPool` 取 `Thread`，拿到后如何组装、驱动、编排，都是各自的事情，与 `ThreadPool` 无关。
8. 本轮非目标：
   - 不改名
   - 不顺手改 desktop monitor / Tauri 协议
   - 不顺手调容量策略、eviction 策略、性能参数

## Key pressure pass

最初用户提出的方向是“`ThreadPool` 放到 `argus-agent` 会更好”。在压力测试中，结合 brownfield 事实发现：

- `argus-agent` 当前被定义为 thread-owned runtime / trace / compact 的事实来源
- 当前 `ThreadPool` 仍携带 runtime 构建/恢复相关依赖

因此直接把完整 `ThreadPool` 并入 `argus-agent` 会扭曲 `argus-agent` 的角色。经过追问后，结论从“迁入 `argus-agent`”改为“拆出独立 `argus-thread-pool` crate”。

## Evidence-backed brownfield facts used during interview

- 根 `CLAUDE.md` 已要求 `argus-job` 做 job 与 runtime pool 解耦，`JobManager` 管 job，`ThreadPool` 只管 chat/runtime 生命周期。
- `crates/argus-job/CLAUDE.md` 也重复了同样边界。
- `docs/plans/2026-04-18-thread-pool-job-decoupling-design.md` 已经把 `ThreadPool` 定义成 chat/runtime-only。
- 但当前 `crates/argus-job/src/thread_pool.rs` 仍返回 `JobError`，并对 `MailboxMessageType::JobResult` 做特殊路由，说明它还未完全去 job 语义。
- 当前 `Session` 很薄，主要是 loaded thread 容器与 broadcast/interrupt 辅助。
- 当前 `JobManager` 已经拥有 job binding、parent/child job thread、result shadow、job runtime summary 等职责。

## Round log

1. 问题：这轮只定边界，还是连 crate 归属一起定？
   回答：`crate 归属也一起定`
2. 问题：若迁到 `argus-agent`，是整包下沉还是只下沉 core？
   回答：`1`
3. 问题：在 `argus-agent` 定位和物理迁入之间如何取舍？
   回答：`换个思路，新增一个 crate argus-thread-pool`
4. 问题：新 crate 的最小职责模型选哪种？
   回答：`A`
5. 问题：是否由 `SessionManager` / `JobManager` 分别构建 runtime 再交给 pool？
   回答：`SessionManager、Job 也要轻量化 —— 应该是从ThreadPool 中拿 Thread ，拿到了之后所有的行为和 ThreadPool 无关！怎么组装是 Session、Job 自己的事情，ThreadPool 只做 Thread 的用量管理与监控`
6. 问题：是否把 rename/UI/perf 一起排除为非目标？
   回答：`不需要改名`
7. 问题：是否也排除 desktop/Tauri 协议和容量策略调参？
   回答：`是`
