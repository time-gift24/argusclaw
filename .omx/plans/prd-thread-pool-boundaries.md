# PRD: Extract `ThreadPool` Into `argus-thread-pool`

## Metadata

- Date: 2026-04-19
- Source spec: `.omx/specs/deep-interview-session-threadpool-job-boundaries.md`
- Planning mode: `ralplan` consensus, short mode
- Scope type: brownfield boundary repair

## Requirements Summary

Refactor the current runtime-pool ownership so `ThreadPool` becomes a pure `Thread` resource-management primitive in a new independent crate `argus-thread-pool`, while `SessionManager` and `JobManager` remain lightweight domain orchestrators that assemble their own chat/job threads and use the pool only for runtime admission, attachment, monitoring, and eviction.

The current codebase shows why the boundary is still wrong:

- There is no standalone pool crate in the workspace yet, so the extraction must introduce a new workspace member in [Cargo.toml](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/Cargo.toml:1).
- `ThreadPool` still lives in `argus-job` and directly owns provider/template/tool/persistence/bootstrap concerns in [crates/argus-job/src/thread_pool.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/thread_pool.rs:160).
- `ThreadPool` still performs session-aware chat loading and message delivery in [crates/argus-job/src/thread_pool.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/thread_pool.rs:416).
- `JobManager` currently constructs the shared pool internally in [crates/argus-job/src/job_manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/job_manager.rs:131), which inverts ownership.
- `ArgusWing` then passes that job-owned pool into `SessionManager` in [crates/argus-wing/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-wing/src/lib.rs:132).
- `argus-session` imports `ThreadPool` from `argus-job` today in [crates/argus-session/Cargo.toml](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/Cargo.toml:7) and [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:9).
- Session-side dependencies on pool-owned chat helpers are broader than just send/load; they include rename, provider updates, snapshots, history, activation, and subscription paths in [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:1040).
- Protocol-facing pool summaries and events still expose `session_id` for compatibility in [crates/argus-protocol/src/events.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-protocol/src/events.rs:228) and [crates/argus-protocol/src/events.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-protocol/src/events.rs:511), so the plan must explicitly separate pool-core APIs from temporary compatibility adapters.

## Acceptance Criteria

1. A new workspace crate `crates/argus-thread-pool` exists and is added to [Cargo.toml](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/Cargo.toml:1).
2. The composition root constructs the shared `ThreadPool` directly and injects it into both `JobManager` and `SessionManager`; `JobManager` no longer constructs the pool internally as it does today in [crates/argus-job/src/job_manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/job_manager.rs:131).
3. `argus-session` imports `ThreadPool` from `argus-thread-pool`, not `argus-job`; `argus-job::ThreadPool` is no longer the source of truth.
4. `ThreadPool` public management APIs in `argus-thread-pool` no longer depend on `TemplateManager`, `ProviderResolver`, `ToolManager`, repository traits, `JobError`, or domain-specific constructors/loaders.
5. `ThreadPool` core APIs no longer branch on chat-vs-job semantics and do not infer `ThreadMessage` types from mailbox content.
6. Session-specific thread assembly, provider resolution, MCP injection, and chat restore/send logic move out of the pool and into session-owned loader/orchestration code, covering current `SessionManager` call sites in [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:620), [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:721), [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:844), and [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:1131).
7. Job-specific thread binding, persistence, restore, and task-execution assembly remain outside the pool, covering the current `JobManager` seams in [crates/argus-job/src/job_manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/job_manager.rs:960) and [crates/argus-job/src/job_manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/job_manager.rs:1411).
8. Shared thread bootstrap/recovery helpers that are not pool-native are explicitly relocated out of the pool. At minimum:
   - `recover_and_validate_metadata`
   - `build_thread_config`
   - `hydrate_turn_log_state`
   - `resolve_provider_with_fallback`
   - `build_chat_thread`
9. Existing protocol-facing monitoring contracts stay behaviorally compatible for this change set, but any `session_id` compatibility is provided by a session-owned adapter/index outside pool-core rather than by polluting the new crate's core ownership model.
10. Verification proves not just that tests pass, but that root wiring and imports changed as intended.
11. `argus-session` owns a minimal `ThreadSessionIndex` or equivalent lookup keyed by `ThreadId`, so mailbox validation/forwarding and compatibility adapters no longer depend on pool summaries for `thread_id -> session_id`.

## RALPLAN-DR Summary

### Principles

1. Fix ownership before relocating code so the new crate reflects a real abstraction, not a renamed mixed one.
2. Keep `ThreadPool` pure by moving both domain assembly and shared bootstrap/recovery helpers out of pool-core.
3. Keep `SessionManager` and `JobManager` lightweight by pushing complexity into owner-local loaders/builders rather than fattening the managers.
4. Preserve user-visible runtime behavior through compatibility adapters and regression tests while the boundary is extracted.

### Decision Drivers

1. `ThreadPool` must stop owning `Session` / `Job` semantics.
2. The crate graph must stop implying that the pool is job-owned or agent-owned.
3. The migration order must avoid temporarily preserving mixed semantics in the new crate.

### Viable Options

The crate destination is already fixed by the source spec. The real remaining options are migration strategies.

#### Option A: Direct crate extraction first

- Approach: move `ThreadPool` into `argus-thread-pool` early, then shrink semantics afterwards.
- Pros: quick ownership signal; compile-driven progress.
- Cons: highest risk of dragging provider/template/persistence/bootstrap helpers into the new crate unchanged.

#### Option B: Staged seam-first extraction

- Approach: invert ownership at the composition root first, split pool-core from bootstrap/recovery seams second, create `argus-thread-pool` third, and only then remove compatibility shims.
- Pros: cleanest final boundary; keeps the new crate honest; reduces semantic rollback risk.
- Cons: more staged plumbing; requires temporary adapters.

#### Option C: Keep `ThreadPool` in `argus-job` and only trim APIs

- Approach: stop at API cleanup without moving the crate.
- Pros: smallest Cargo churn.
- Cons: still communicates the wrong ownership and contradicts the clarified source spec.

### Recommended Option

Choose **Option B**.

### Invalidation Rationale For Rejected Options

- Option A is invalid because the current `ThreadPool` still owns too much bootstrap and persistence logic, so “move first, purify later” is likely to preserve the wrong abstraction under a cleaner name.
- Option C is invalid because crate ownership would remain misleading even if the public API got smaller.

## ADR

### Decision

Use a staged extraction strategy: invert shared pool ownership at the composition root, split pool-core from non-pool helpers, then move only pure pool semantics into the new `argus-thread-pool` crate.

### Drivers

- The deep-interview source spec already fixed the destination crate and the “no Session/Job semantics” boundary.
- Architect and critic review both identified the real risk as seam leakage, not just wrong crate placement.
- Current protocol surfaces still carry `session_id`, so compatibility needs an adapter strategy rather than an all-at-once protocol rewrite.

### Alternatives Considered

- Direct crate extraction first.
- Keep the pool inside `argus-job`.

### Why Chosen

It is the only approach that simultaneously honors the source spec, fixes the actual ownership inversion, and avoids creating a new crate full of old responsibilities.

### Consequences

- `ArgusWing` or another top-level composition root becomes the owner/injector of the shared pool.
- `argus-session` and `argus-job` both depend directly on `argus-thread-pool`.
- Session/job-specific thread loaders become explicit owner-local abstractions.
- A compatibility adapter is required temporarily for current protocol/monitoring fields such as `session_id`.

### Follow-ups

- Keep shared bootstrap helpers consolidated under `argus-agent::thread_bootstrap` and resist re-spreading them into pool or manager code during execution.
- Decide when to run a separate protocol cleanup pass after this extraction lands.
- Audit all remaining imports and tests for old root wiring assumptions.

## Seam Map

### Stays In `argus-thread-pool` Core

- runtime registry/store
- slot admission and waiter tracking
- attach/detach of already-built `Thread`
- lifecycle transitions: queued/running/cooling/evicted
- metrics collection and runtime lifecycle observer hooks
- generic runtime removal/shutdown behavior

### Moves Out Of Pool-Core Into Session-Owned Code

- chat-thread construction and restoration currently rooted in `build_chat_thread` and `ensure_chat_runtime`
- provider resolution fallback for chat runtime creation
- MCP resolver injection
- chat message send path and mailbox routing
- session-owned in-memory thread lookup/update behavior currently tied to `loaded_chat_thread`

### Moves Out Of Pool-Core Into Job-Owned Code

- job binding persistence and recovery
- job-thread construction/restoration
- task-assignment delivery semantics
- job runtime state projection and persistence
- job-side trace directory lookup and metadata repair

### Moves Out Of Pool-Core Into Shared Thread Bootstrap Helpers

These helpers are about thread bootstrap/trace semantics, not pool ownership:

- `recover_and_validate_metadata`
- `build_thread_config`
- `hydrate_turn_log_state`
- trace cleanup / trace metadata helpers currently reused across owners

Named home:

- `argus-agent::thread_bootstrap` (or an equivalently named module inside `argus-agent`) because `argus-agent` already owns thread trace/log/config semantics. This module must stay free of template/provider/repository/session/job orchestration concerns.

## Implementation Steps

### Step 1: Lock current behavior with characterization tests

- Add or move regression tests that prove:
  - current pool metrics/lifecycle behavior
  - session chat load/send/history/snapshot/activation behavior
  - job dispatch/cancel/complete/cooling behavior
  - current wing-level `session_id` monitoring compatibility assumptions in [crates/argus-wing/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-wing/src/lib.rs:1578)
- Lock both `ArgusWing::init()` and `ArgusWing::with_pool()` wiring parity per [crates/argus-wing/CLAUDE.md](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-wing/CLAUDE.md:1) and the duplicated setup paths in [crates/argus-wing/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-wing/src/lib.rs:89) and [crates/argus-wing/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-wing/src/lib.rs:184).

### Step 2: Invert ownership at the composition root

- Change the root wiring in [crates/argus-wing/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-wing/src/lib.rs:132) so the shared pool is created independently and injected into both managers.
- Remove `JobManager`'s responsibility for constructing the shared pool from [crates/argus-job/src/job_manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/job_manager.rs:113).

### Step 3: Split helper seams before any crate move

- Move pool-non-native helpers out of `crates/argus-job/src/thread_pool.rs` first.
- Create owner-local session/job loaders or builders so `SessionManager` / `JobManager` stay lightweight and do not absorb all bootstrap details inline.
- Introduce `argus-agent::thread_bootstrap` for thread trace/config hydration helpers that both owners can call without routing through the pool.
- Add a small session-owned `ThreadSessionIndex` in `argus-session` to own `thread_id -> session_id` lookup for:
  - mailbox target validation currently using `runtime_summary().session_id` in [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:272)
  - mailbox forwarding currently using pool summaries in [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:627)
  - protocol compatibility adapters for `session_id`-bearing thread-pool summaries/events

### Step 4: Introduce `argus-thread-pool` and move only pool-core

- Add the crate to [Cargo.toml](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/Cargo.toml:1).
- Move pure pool-core types and APIs into `crates/argus-thread-pool/src/`.
- Ensure the new crate constructor and public API are free of template/provider/tool/repository dependencies.
- After the crate exists, update `argus-session` imports so it no longer obtains `ThreadPool` through `argus-job`.

### Step 5: Rewire session-owned thread orchestration

- Update session constructor wiring and mailbox-forwarder integration currently rooted in [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:620).
- Replace pool-owned chat helpers used across:
  - `load`
  - `create_thread`
  - `rename_thread`
  - `update_thread_model`
  - `send_message`
  - `cancel_thread`
  - `get_thread_messages`
  - `get_thread_snapshot`
  - `activate_thread`
  - `subscribe`
- Keep `Session` itself thin as a loaded-thread container in [crates/argus-session/src/session.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/session.rs:29).

### Step 6: Rewire job-owned thread orchestration

- Update job binding and restoration seams currently rooted in [crates/argus-job/src/job_manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/job_manager.rs:1411).
- Keep the runtime lifecycle bridge model from [crates/argus-job/src/job_manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/job_manager.rs:436), but retarget it to the new pool crate.
- Convert job delivery so callers choose `ThreadMessage` semantics; the pool only forwards what it is given.

### Step 7: Add compatibility adapters and remove stale APIs

- Preserve protocol-facing `ThreadPoolRuntimeSummary.session_id` and `ThreadPool*` event compatibility through an adapter layer outside pool-core for this change set.
- Back the compatibility adapter with the session-owned `ThreadSessionIndex`, not with pool summaries that know session ownership.
- Remove old pool APIs that encode session-specific ownership:
  - `loaded_chat_thread`
  - `send_chat_message`
  - `ensure_chat_runtime`
  - session-aware `runtime_summary` assumptions
  - mailbox content inspection used to infer `JobResult`
- Remove `JobError` from pool APIs.

### Step 8: Final cleanup and verification

- Remove old re-exports/imports through `argus-job`.
- Confirm the composition root, `argus-session`, and `argus-job` all wire through the new crate directly.
- Run the full verification suite and import-guard checks.

## Risks And Mitigations

- Risk: a direct move leaves bootstrap helpers inside the new crate.
  - Mitigation: helper-seam split is a required phase before crate extraction.
- Risk: `SessionManager` or `JobManager` become fat god-objects.
  - Mitigation: use owner-local loaders/builders and keep manager methods orchestration-focused.
- Risk: `session_id` compatibility becomes a hidden excuse for reintroducing session semantics into pool-core.
  - Mitigation: treat `session_id` as adapter-only compatibility in this plan and verify that the new crate core APIs do not require it.
- Risk: root wiring remains inverted, so ownership is still effectively job-owned.
  - Mitigation: composition-root inversion is the first migration gate and a final verification checkpoint.

## Verification Steps

1. `cargo test -p argus-thread-pool`
2. `cargo test -p argus-session`
3. `cargo test -p argus-job`
4. `cargo test -p argus-wing`
5. `cargo test`
6. Import/root wiring checks:
   - `rg -n "argus_job::.*ThreadPool|use argus_job::\\{[^}]*ThreadPool" crates`
   - `rg -n "thread_pool\\(\\)" crates/argus-wing/src/lib.rs crates/argus-job/src/job_manager.rs`
7. Pool-core dependency guard checks:
   - `rg -n "TemplateManager|ProviderResolver|ToolManager|ThreadPoolPersistence|JobError|SessionId|ThreadRepository|LlmProviderRepository|JobRepository" crates/argus-thread-pool/src`
8. Compatibility regression checks for the chosen monitoring stance:
   - keep existing wing tests around `thread_pool_state().runtimes[*].session_id` green, or replace them with equivalent adapter-level assertions if file moves require it
9. Root parity checks:
   - verify both `ArgusWing::init()` and `ArgusWing::with_pool()` construct the same manager/pool graph after the extraction

## Available-Agent-Types Roster

- `architect`: boundary design and seam validation
- `executor`: crate extraction and call-site rewiring
- `critic`: plan review and consistency checks
- `verifier`: completion evidence review
- `test-engineer`: regression test design and verification
- `build-fixer`: compile fallout and dependency cleanup
- `debugger`: runtime regressions during extraction
- `code-reviewer`: final review of boundary polish
- `explore`: read-only codebase mapping

## Follow-up Staffing Guidance

### `$ralph` path

- Leader: `executor` at high reasoning
- Required review checkpoints:
  - `architect` high reasoning after Step 3 seam split
  - `test-engineer` medium reasoning before Step 7 compatibility cleanup
  - `verifier` high reasoning for final evidence
- Best when: one owner should preserve boundary intent across a staged extraction.

### `$team` path

- Lane 1: `executor` high reasoning
  - Own composition-root inversion and new crate creation.
- Lane 2: `executor` high reasoning
  - Own session-owned loader/orchestration migration.
- Lane 3: `executor` high reasoning
  - Own job-owned loader/orchestration migration.
- Lane 4: `test-engineer` medium reasoning
  - Own characterization tests, import guards, and compatibility assertions.
- Lane 5: `build-fixer` high reasoning
  - Own compile fallout, workspace dependency rewiring, and re-export cleanup.

## Launch Hints

### Ralph

```text
$ralph .omx/plans/prd-thread-pool-boundaries.md
```

### Team

```text
$team .omx/plans/prd-thread-pool-boundaries.md
```

or:

```text
omx team run .omx/plans/prd-thread-pool-boundaries.md
```

## Team Verification Path

Before team shutdown:

1. Root wiring lane proves the pool is no longer created inside `JobManager`.
2. Pool lane proves the new crate exports only pool-core semantics.
3. Session lane proves chat assembly no longer lives in pool-core and `argus-session` no longer imports `ThreadPool` from `argus-job`.
4. Job lane proves job assembly no longer lives in pool-core.
5. Test lane proves compatibility assertions and import-guard checks pass.

After team handoff, a final Ralph/verifier pass should confirm:

1. No remaining `argus_job::ThreadPool` import path exists.
2. No pool-core API still requires template/provider/tool/repository/job error dependencies.
3. The monitoring compatibility stance is explicit and tested.
4. Workspace tests pass for all affected crates.

## Changelog

- Revised after architect + critic feedback.
- Switched from “direct extraction” framing to “staged seam-first extraction”.
- Resolved the `SessionId` contradiction by separating pool-core APIs from temporary compatibility adapters.
- Expanded the session migration inventory and verification gates.
