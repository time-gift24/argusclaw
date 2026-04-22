# Context Snapshot

- Task statement:
  Confirm the responsibility boundaries among `Session`, `ThreadPool`, and `Job`, and pressure-test whether the current design is redundant/inefficient, especially whether `ThreadPool` should be fully session/job-agnostic and possibly live in `argus-agent`.
- Desired outcome:
  Produce an execution-ready boundary spec for the three concepts, including ownership, non-goals, and decision boundaries for a later planning/execution handoff.
- Stated solution:
  User hypothesis: `ThreadPool` is the lowest layer, should be unaware of `Session` and `Job`, and may belong in `argus-agent`.
- Probable intent hypothesis:
  Reduce architectural redundancy and wrong ownership before more implementation calcifies the current coupling.
- Known facts / evidence:
  - Root [CLAUDE.md](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/CLAUDE.md:72) says `argus-job` should decouple background job lifecycle from runtime pool: `JobManager` owns jobs, `ThreadPool` owns chat/runtime lifecycle.
  - [crates/argus-job/CLAUDE.md](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/CLAUDE.md:3) repeats that `ThreadPool` should only handle runtime registration/loading/eviction/snapshot/generic mailbox delivery.
  - [docs/plans/2026-04-18-thread-pool-job-decoupling-design.md](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/docs/plans/2026-04-18-thread-pool-job-decoupling-design.md:7) explicitly targets `ThreadPool` as chat/runtime-only and `JobManager` as sole job owner.
  - Current [crates/argus-job/src/thread_pool.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/thread_pool.rs:409) still routes `MailboxMessageType::JobResult` specially and returns `JobError`, so the pool is not fully job-neutral yet.
  - Current [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:115) still exposes `ThreadPool` through session orchestration for runtime loading/delivery.
  - `Session` itself is currently a thin in-memory container of loaded threads plus broadcast/interrupt helpers in [crates/argus-session/src/session.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/session.rs:29).
  - `JobManager` already owns job bindings, parent/child job-thread relations, delivered result shadow, and job runtime summaries in [crates/argus-job/src/job_manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/job_manager.rs:58).
  - `argus-agent` is documented as thread-owned turn runtime / trace / compact owner, not orchestration/pool owner, in [crates/argus-agent/CLAUDE.md](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-agent/CLAUDE.md:3).
- Constraints:
  - This is a `deep-interview` requirements turn; no direct implementation.
  - Need to follow nearest `CLAUDE.md` / `AGENTS.md` files.
  - Repository guidance prefers explaining problems in terms of responsibility boundaries and state models, not patch symptoms.
- Unknowns / open questions:
  - Is the desired end state only boundary clarification, or also crate relocation (`ThreadPool` out of `argus-job`) in scope?
  - Should `ThreadPool` remain aware of chat-vs-job runtime kinds, or become a pure runtime slot/lifecycle primitive with all semantic routing above it?
  - Is the main optimization target conceptual clarity, dependency graph simplification, runtime performance, or recovery correctness?
- Decision-boundary unknowns:
  - Can OMX later choose to keep `ThreadPool` in `argus-job` if responsibilities are clean, or is physical crate ownership part of the required outcome?
  - Can OMX propose renaming `ThreadPool` to `RuntimePool` as part of the boundary cleanup, or must naming stay fixed?
- Likely codebase touchpoints:
  - `crates/argus-job/src/thread_pool.rs`
  - `crates/argus-job/src/job_manager.rs`
  - `crates/argus-session/src/session.rs`
  - `crates/argus-session/src/manager.rs`
  - `crates/argus-agent/src/thread.rs`
  - `docs/plans/2026-04-17-thread-routing-design.md`
  - `docs/plans/2026-04-18-thread-pool-job-decoupling-design.md`
