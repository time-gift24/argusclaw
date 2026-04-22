# Deep Interview Spec: Session / ThreadPool / Job Boundaries

## Metadata

- Date: 2026-04-19T15:44:42Z
- Profile: standard
- Rounds: 7
- Final ambiguity: 0.11
- Threshold: 0.20
- Context type: brownfield
- Context snapshot: `.omx/context/session-threadpool-job-boundaries-20260419T031842Z.md`
- Transcript: `.omx/interviews/session-threadpool-job-boundaries-20260419T154442Z.md`

## Clarity Breakdown

| Dimension | Score |
| --- | --- |
| Intent | 0.90 |
| Outcome | 0.93 |
| Scope | 0.91 |
| Constraints | 0.88 |
| Success Criteria | 0.76 |
| Context | 0.91 |

Brownfield ambiguity formula:

`1 - (intent*0.25 + outcome*0.20 + scope*0.20 + constraints*0.15 + success*0.10 + context*0.10) = 0.11`

## Intent

用户要解决的核心不是局部重复代码，而是职责边界错位：

- `ThreadPool` 不应该承载 `Session` 或 `Job` 语义
- `SessionManager`、`JobManager` 也不该变成厚重装配层
- crate 归属必须反映真实边界，避免继续把基础设施层伪装成 job 层

目标是把 runtime pool 收敛成真正的基础设施层，把会话编排和 job 编排留在各自上层。

## Desired Outcome

确定以下终态作为后续规划/实现的源事实：

1. 新增独立 crate：`argus-thread-pool`
2. `ThreadPool` 保留现有名称，不做 rename pass
3. `ThreadPool` 是纯 `Thread` runtime pool 基础设施
4. `ThreadPool` 只负责 `Thread` 的用量管理与监控
5. `ThreadPool` 禁止感知 `Session`、`Job` 语义
6. `SessionManager` 和 `JobManager` 都应轻量化
7. `SessionManager` / `JobManager` 只从 `ThreadPool` 拿 `Thread`
8. 一旦拿到 `Thread`，之后的装配、行为驱动、编排和生命周期语义都属于 `SessionManager` / `JobManager` 自己，不再属于 `ThreadPool`

## In Scope

- 明确 `Session`、`ThreadPool`、`Job` 的职责边界
- 明确 `ThreadPool` 的 crate 归属
- 明确新 crate 的最小职责模型
- 明确 `SessionManager` / `JobManager` 与 `ThreadPool` 的装配边界
- 明确本轮非目标

## Out of Scope / Non-goals

- 不改 `ThreadPool` 名称
- 不顺手做 `ThreadPool -> RuntimePool` 命名收口
- 不顺手改 desktop monitor / Tauri 协议
- 不顺手调容量策略、eviction 策略、性能参数
- 不在本规格里直接进入实现

## Decision Boundaries

以下事项 OMX 在后续 `ralplan` / 执行阶段可自行决定，无需再次确认：

- `argus-thread-pool` 的具体模块拆分和 API 形状
- `SessionManager` / `JobManager` 通过什么 builder / factory / adapter 组装 `Thread`
- 迁移顺序与中间兼容层
- 现有 `ThreadPool` 中哪些 helper 应迁往 `SessionManager`，哪些应迁往 `JobManager`
- 如何把 `ThreadPool` 的监控与容量控制接口最小化

以下事项已被拍板，不应在后续规划中被悄悄改写：

- 必须新增独立 crate `argus-thread-pool`
- `ThreadPool` 不并入 `argus-agent`
- `ThreadPool` 不是装配层，不做 `Session` / `Job` 语义判断
- `ThreadPool` 不做 runtime 语义编排，只做 `Thread` 用量管理与监控
- 不做 rename pass

## Constraints

- `ThreadPool` 必须从 `argus-job` 中抽离成独立 crate
- `ThreadPool` 不能依赖 `Session` / `Job` 概念
- `ThreadPool` 不应继续持有会让它承担上层语义的依赖：
  例如 `TemplateManager`、`ProviderResolver`、`ToolManager`、repository 访问、`JobError`
- `SessionManager` / `JobManager` 应轻量化，不要把 pool 内部控制逻辑重新复制到上层
- crate 边界要与仓库文档里“职责归属”的表达一致

## Testable Acceptance Criteria

1. 仓库内出现独立 crate：`argus-thread-pool`
2. `ThreadPool` 的对外职责只剩 `Thread` runtime pool 相关能力：
   admission、attach、usage tracking、monitoring、cooling、eviction，以及必要的 pool-level delivery primitive
3. `ThreadPool` 的公开 API 不再暴露 `SessionId` / `JobId` 语义，不再内建 chat-vs-job 语义分叉
4. `ThreadPool` 不再直接持有 `TemplateManager`、`ProviderResolver`、`ToolManager`、repository 访问或 `JobError`
5. `SessionManager` 负责 session/chat 侧的 `Thread` 装配与编排
6. `JobManager` 负责 job 侧的 `Thread` 装配与编排
7. `SessionManager` / `JobManager` 从 `ThreadPool` 获取 `Thread` 后，后续行为与 `ThreadPool` 无关
8. 本轮实现不包含 rename、desktop/Tauri 协议调整、容量策略/性能调参

## Assumptions Exposed And Resolutions

| Exposed assumption | Resolution |
| --- | --- |
| `ThreadPool` 放到 `argus-agent` 会更好 | 否。这样会扭曲 `argus-agent` 作为 thread-owned runtime crate 的定位 |
| 可以保留“完整 ThreadPool”但只换 crate | 否。新 crate 需要选择更小的基础设施边界 |
| `ThreadPool` 仍可兼任 runtime 构建/恢复 | 否。用户希望 `ThreadPool` 只做用量管理与监控，不做装配层 |

## Pressure-pass Findings

被 revisited 的核心答案是：

- 初始方向：`ThreadPool` 放到 `argus-agent`

压力测试问题：

- 如果整包迁入 `argus-agent`，是否接受把 `argus-agent` 从 thread-owned runtime crate 升格为 runtime orchestration crate？

结果变化：

- 用户放弃“迁入 `argus-agent`”
- 改为“新增独立 crate `argus-thread-pool`”
- 再进一步收口为“纯基础设施层，只做 `Thread` 用量管理与监控”

## Brownfield Evidence Vs Inference

### Evidence

- 根 `CLAUDE.md` 已明确：`JobManager` 管 job，`ThreadPool` 只管 chat/runtime 生命周期
- `crates/argus-job/CLAUDE.md` 重复了同样边界
- `docs/plans/2026-04-18-thread-pool-job-decoupling-design.md` 也已经把 `ThreadPool` 目标定义成 chat/runtime-only
- 当前 `crates/argus-job/src/thread_pool.rs` 仍带有 `JobError` 和 `JobResult` 路由痕迹，说明实现边界未完全收口
- 当前 `crates/argus-session/src/session.rs` 显示 `Session` 本体已经相对薄
- 当前 `crates/argus-job/src/job_manager.rs` 显示 `JobManager` 已拥有大量 job 生命周期与 runtime state 职责

### Inference

- 如果继续把 `ThreadPool` 留在 `argus-job`，crate 语义会继续误导
- 如果把完整 `ThreadPool` 并入 `argus-agent`，会扩大 `argus-agent` 职责，破坏现有分层叙事
- 因此独立 crate `argus-thread-pool` 是与当前仓库边界最一致的方向

## Technical Context Findings

- 当前 `ThreadPool` 仍包含 chat runtime 构建/恢复逻辑
- 当前 `JobManager` 主要负责 job runtime 构建/恢复、job binding、job runtime summary
- 当前 `SessionManager` 仍通过 `ThreadPool` 承担部分 chat runtime ensure/load 逻辑
- 这意味着后续实现需要把“装配 `Thread`”与“管理 `Thread` 用量/监控”拆成两层

## Recommended Handoff

推荐下一步进入：

`$ralplan .omx/specs/deep-interview-session-threadpool-job-boundaries.md`

推荐原因：

- 需求和边界已经清楚
- 还需要把 crate 拆分顺序、依赖回迁、测试切分和迁移风险设计清楚
- 这一步比直接进实现更稳，尤其能避免在“Session/Job 也要轻量化”这点上又反弹成上层肥大

## Condensed Transcript

1. 用户要求这轮把职责边界和 crate 归属一起定
2. 先尝试把 `ThreadPool` 放到 `argus-agent`
3. 经压力测试后改为新增 `argus-thread-pool`
4. 明确新 crate 选最小职责模型 A
5. 明确 `SessionManager` / `JobManager` 轻量化，只从 `ThreadPool` 取 `Thread`
6. 明确 `ThreadPool` 只做 `Thread` 用量管理与监控
7. 明确不改名、不做 UI 协议改动、不做性能策略调参
