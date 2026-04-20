# Test Spec: `ThreadPool` Boundary Extraction

## Goal

Prove that the `ThreadPool` extraction into `argus-thread-pool` preserves runtime behavior while removing session/job semantic ownership from pool-core.

## Contract Stance For This Change Set

- `ThreadPool` core APIs in the new crate must be free of `Session` / `Job` semantics.
- Existing protocol-facing monitoring fields such as `ThreadPoolRuntimeSummary.session_id` may remain temporarily for compatibility, but only through an adapter layer outside pool-core.
- This change set does not redesign desktop/Tauri protocol contracts; it only preserves or adapts them.

## Test Surfaces

### 1. Pool-core behavior

Target crate:

- `argus-thread-pool`

Must verify:

- slot admission and waiter behavior still work
- attach/detach lifecycle works for already-built `Thread` instances
- running/cooling/eviction transitions still emit expected pool metrics and lifecycle notifications
- delivery primitives forward caller-chosen `ThreadMessage` values without inferring chat/job semantics from mailbox payloads
- core APIs do not require `TemplateManager`, `ProviderResolver`, `ToolManager`, repository traits, `SessionId`, or `JobError`

### 2. Composition-root ownership inversion

Target crates:

- `argus-wing`
- `argus-job`
- `argus-session`

Must verify:

- the shared pool is no longer constructed inside `JobManager`
- the composition root injects the shared pool into both managers
- `argus-session` no longer imports `ThreadPool` from `argus-job`
- `ArgusWing::init()` and `ArgusWing::with_pool()` produce equivalent manager/pool wiring

### 3. Session flow regression

Target crate:

- `argus-session`

Must verify:

- session loading still restores chat threads
- session-owned code, not pool-core, performs chat-thread build/restore and MCP resolver injection
- a session-owned `ThreadSessionIndex` or equivalent lookup now owns `thread_id -> session_id` resolution for mailbox validation/forwarding and compatibility adapters
- message send/cancel paths still reach the target thread after extraction
- rename/update/history/snapshot/activation/subscribe paths no longer depend on session-specific pool helpers such as `loaded_chat_thread` or `ensure_chat_runtime`
- `Session` remains a thin container over loaded `Thread` handles

### 4. Job flow regression

Target crate:

- `argus-job`

Must verify:

- job dispatch still admits/attaches a runtime and executes the task-assignment turn
- job cancellation still forwards interrupt semantics correctly
- job completion still updates job runtime summaries and persistence
- job-thread construction/restoration is owned by `JobManager` or job-owned helpers, not by pool-core
- runtime lifecycle bridge behavior still works against the new pool crate

### 5. Compatibility adapter regression

Target crate:

- `argus-wing` and any adapter surface chosen during implementation

Must verify:

- existing `thread_pool_state()`/event consumers still observe the expected `session_id` compatibility where the current tests require it
- compatibility is produced outside pool-core

## Required Test Cases

1. Pool admit + attach + running + cooling + evict happy path
2. Pool slot exhaustion / wait behavior regression
3. Pool delivery regression using caller-supplied `ThreadMessage::UserInput`, `PeerMessage`, and `JobResult`
4. Compile-time or import-guard regression proving no forbidden pool-core dependencies remain
5. Composition-root regression proving the shared pool is not created by `JobManager`
6. Root parity regression proving `ArgusWing::init()` and `ArgusWing::with_pool()` wire the same pool/manager graph
7. Session load regression for restored chat thread startup
8. Session send/cancel regression after pool extraction
9. Session rename/update/history/snapshot/activation regression after session-owned loader migration
10. Session mailbox validation/forwarding regression using the new `ThreadSessionIndex`
11. Job execution regression covering runtime attach, task assignment delivery, and completion capture
12. Job cancellation regression covering interrupt forwarding and terminal runtime reason
13. Adapter regression preserving current `session_id` expectations in `ArgusWing` tests or their updated equivalent

## Verification Commands

1. `cargo test -p argus-thread-pool`
2. `cargo test -p argus-session`
3. `cargo test -p argus-job`
4. `cargo test -p argus-wing`
5. `cargo test`
6. `rg -n "argus_job::.*ThreadPool|use argus_job::\\{[^}]*ThreadPool" crates`
7. `rg -n "TemplateManager|ProviderResolver|ToolManager|ThreadPoolPersistence|JobError|SessionId" crates/argus-thread-pool/src`

## Test Design Notes

- Move existing lifecycle tests into `argus-thread-pool` only after the seam split is done, so the tests validate the final abstraction rather than the old mixed one.
- Add characterization tests before deleting session-specific pool helpers.
- Keep or replace the existing `ArgusWing` compatibility assertions around `runtime.session_id` from [crates/argus-wing/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-wing/src/lib.rs:1578) and [crates/argus-wing/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-wing/src/lib.rs:1650), depending on where the compatibility adapter lands.
- Keep `ThreadSessionIndex` intentionally tiny: ownership lookup only, no pool metrics or runtime lifecycle state.
- If grep-based dependency guards are too blunt, replace them with compile-fail or narrower API-surface assertions, but keep the same intent.

## Exit Criteria

- All required commands pass.
- `argus-thread-pool` owns only pool-core tests.
- Session/job regressions prove orchestration moved outward while pool-core behavior stayed stable.
- Root wiring and imports prove the pool is no longer job-owned.
- Compatibility behavior is explicit, intentional, and tested.
