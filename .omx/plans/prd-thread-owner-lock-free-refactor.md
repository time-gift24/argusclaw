# PRD: Lock-Free Thread Owner/Observer Redesign

## Metadata

- Date: 2026-04-20
- Source specs:
  - `.omx/specs/deep-interview-argus-agent-thread-core-lock-free.md`
  - `.omx/specs/deep-interview-session-threadpool-job-boundaries.md`
- Planning mode: `ralplan` consensus re-review after Architect `ITERATE` and Critic `REJECT`
- Scope type: brownfield lock-free runtime redesign

## Re-anchored Brownfield State

These repo facts are already true and must be treated as current state, not future work:

- Workspace already includes `crates/argus-thread-pool` in [Cargo.toml](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/Cargo.toml:1).
- `argus-session` already depends on `argus-thread-pool` in [crates/argus-session/Cargo.toml](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/Cargo.toml:1).
- `ArgusWing` already constructs the shared `ThreadPool` at the composition root and injects it into both managers in [crates/argus-wing/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-wing/src/lib.rs:150) and [crates/argus-wing/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-wing/src/lib.rs:235).
- `SessionManager` already owns a `thread_sessions` adapter/index in [crates/argus-session/src/manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/manager.rs:127) and uses it for mailbox/session lookups.

The remaining problem is the runtime ownership model, not crate extraction:

- `Thread` reactor still starts as `spawn_reactor(thread: Arc<RwLock<Self>>)` in [crates/argus-agent/src/thread.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-agent/src/thread.rs:573).
- `argus-thread-pool` still stores and returns `Arc<RwLock<Thread>>` in [crates/argus-thread-pool/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-thread-pool/src/lib.rs:61), [crates/argus-thread-pool/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-thread-pool/src/lib.rs:196), and [crates/argus-thread-pool/src/lib.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-thread-pool/src/lib.rs:523).
- `argus-session` still exposes chat/session flows through `Arc<RwLock<Thread>>`, including `Weak<RwLock<Thread>>` session caching in [crates/argus-session/src/session.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-session/src/session.rs:33).
- `argus-job` still reads thread internals through pool-returned shared ownership for trace-path, labels, and runtime inspection in [crates/argus-job/src/job_manager.rs](/Users/wanyaozhong/projects/argusclaw/.worktrees/codex-thread-router-design/crates/argus-job/src/job_manager.rs:1696).

## Problem Statement

The codebase has already moved pool ownership to the right crate and root wiring, but the runtime still behaves as if `Thread` were a shared mutable object. That violates the source-of-truth lock-free task:

1. `Thread` must become a single-owner runtime.
2. Cross-crate callers must stop passing or storing `Arc<RwLock<Thread>>`.
3. Observer reads must move to mailbox, event, snapshot, or query surfaces with eventual consistency.
4. Persisted metadata and owner-side recovery must remain authoritative for parent/child/session/job relationships.
5. Tests must stop expressing the runtime through shared-lock wrappers.

## Target Runtime Contract

### Owner Contract

- A loaded thread runtime has exactly one mutable owner task inside `argus-agent`.
- That owner task exclusively mutates runtime state, queued messages, active turn cancellation, turn settlement, trace-side live state, and transient summary caches.
- The owner task may use single-writer-friendly coordination primitives such as `mpsc`, `broadcast`, atomics, or internal task-local state.
- The owner task is responsible for updating any exported snapshots/caches after state transitions.

### External Handle Contract

`Arc<RwLock<Thread>>` is replaced by an explicit runtime handle contract. Exact names remain implementation choice, but the contract is fixed:

- `ThreadOwner` or equivalent:
  - non-cloneable runtime owner object
  - owns the reactor loop and mutable `Thread` state
- `ThreadHandle` or equivalent:
  - cheap cloneable observer/control handle
  - contains only message ingress, event subscription, and query/snapshot access
  - does not expose `&Thread`, `&mut Thread`, lock guards, or shared object identity
- Optional `ThreadSnapshot` / `ThreadRuntimeView`:
  - immutable read model returned directly from owner-managed cache/query surfaces
  - may include title, provider/model label, runtime status, turn counts, estimated memory, last-active time, trace path, and settled message summaries

### Explicit Prohibitions

The redesign must not accept any of the following disguised regressions:

- `Arc<RwLock<Thread>>` renamed to `ThreadHandle`, `ThreadRuntime`, `SharedThread`, or similar.
- A wrapper type whose main payload is still `Arc<RwLock<Thread>>`.
- Returning lock guards, `Weak<RwLock<Thread>>`, or helper closures that grant direct read/write access to the shared thread object.
- Pool, session, or job APIs that preserve shared runtime ownership under a new type alias.

### Query And Snapshot Surfaces

Required read surfaces under the new model:

- mailbox/control send surface
- event subscription surface
- runtime summary surface for pool metrics and lifecycle
- session-owned lookup surface for `thread_id -> session_id`
- owner-backed label lookup for human-readable source labels
- owner-backed memory estimation surface
- owner-backed trace-path read surface
- session-facing history/snapshot surface for UI/API reads

### Consistency Contract

- Reads are eventually consistent, not strongly consistent.
- Query/snapshot responses may lag a just-completed mutation or turn transition.
- Parent/child linkage, root-session lineage, job lineage, and recoverability decisions must not derive authority from stale summaries.
- If a query cannot be answered from current cache/snapshot state, the owner-side recovery path or persisted metadata is the authority.

## Authority Boundaries

### Authoritative

- persisted thread metadata
- owner-side recovery/bootstrap logic
- repository-backed session/thread/job records
- owner-maintained runtime state for the currently loaded thread
- job-side recovery metadata for parent/child and trace lineage

### Cache / Adapter Only

- `argus-thread-pool` runtime summaries and pool snapshots
- `argus-session` `thread_sessions` index
- generic `ThreadHandle` query caches
- wing-facing summaries that include compatibility fields

The pool, session index, and generic handles must never become the authority for parent/child/session/job relationships.

## Scope

### In Scope

- redefine the runtime owner/observer contract in `argus-agent`
- remove `Arc<RwLock<Thread>>` and `Weak<RwLock<Thread>>` from main runtime paths in `argus-agent`, `argus-thread-pool`, `argus-session`, and `argus-job`
- move read access onto explicit query/snapshot surfaces
- keep pool boundaries aligned with current repo state
- make migration seams first-class for bootstrap/hydration, idle hooks, memory hooks, session behavior, and tests

### Out Of Scope

- rewriting repository/trace persistence as an independent project
- changing `argus-thread-pool` crate ownership or re-extracting it
- desktop/Tauri protocol redesign beyond adapting to new read surfaces
- chasing absolute zero-lock usage outside the shared-runtime ownership problem

## Acceptance Criteria

1. `spawn_reactor(Arc<RwLock<Self>>)` is eliminated from `argus-agent`, and the reactor runs from a single-owner runtime entrypoint.
2. `Arc<RwLock<Thread>>`, `Weak<RwLock<Thread>>`, and equivalent shared-runtime wrappers are absent from the primary runtime paths of `argus-agent`, `argus-thread-pool`, `argus-session`, and `argus-job`.
3. No new handle type is a renamed shared-object wrapper; residue checks explicitly forbid that regression.
4. `argus-thread-pool` accepts and stores runtime handles/snapshots instead of shared thread objects.
5. Pool lifecycle, idle hooks, and memory-estimation hooks read through owner-backed query surfaces rather than direct `thread.read().await`.
6. Session list/history/snapshot/broadcast/subscription flows operate through handles and owner-backed snapshots, not direct thread locks.
7. Session ownership validation continues to use `thread_sessions` as an adapter only; persisted metadata and owner-side recovery remain authoritative.
8. Job label lookup, trace-path lookup, and parent/child lineage checks use owner-backed queries plus persisted metadata, not shared runtime ownership.
9. Parent/child/session/job relationship authority remains persisted metadata or owner-side recovery; pool summaries and generic handles are documented and tested as caches only.
10. No-lock test replacements cover runtime unit tests, integration tests, and boundary tests so the test suite no longer encodes the shared-lock model.

## Migration Phases And File Ownership

### Phase 1: Owner/Handle Core In `argus-agent`

Files:

- `crates/argus-agent/src/thread.rs`
- `crates/argus-agent/src/thread_bootstrap.rs`
- `crates/argus-agent/tests/trace_integration_test.rs`

Plan:

- replace `spawn_reactor(Arc<RwLock<Self>>)` with an owner-driven reactor bootstrap
- define the handle/query/snapshot contract
- keep bootstrap and recovery logic owner-adjacent, not pool-owned
- migrate trace integration tests away from `Arc<RwLock<Thread>>`

Exit gate:

- `argus-agent` can load, run, query, and stop a thread without shared runtime locks

### Phase 2: Pool Runtime Residency And Hooks

Files:

- `crates/argus-thread-pool/src/lib.rs`

Plan:

- replace stored `Arc<RwLock<Thread>>` with the new runtime handle/snapshot model
- convert `register_runtime`, `loaded_runtime`, idle-settle, memory estimation, and shutdown/delivery paths to handle/query APIs
- preserve current crate wording: pool already exists; only runtime handle semantics change

Exit gate:

- pool no longer owns or returns shared runtime objects

### Phase 3: Session Cache, List, Broadcast, History, Snapshot

Files:

- `crates/argus-session/src/manager.rs`
- `crates/argus-session/src/session.rs`

Plan:

- replace `Weak<RwLock<Thread>>` session cache with owner/handle references appropriate to the new contract
- keep `thread_sessions` as session-owned adapter/index only
- migrate loaded-thread lookup, source-label reads, send/cancel, history, snapshot, subscription, activation, and list behavior to handle/query APIs
- keep mailbox/session authorization behavior unchanged

Exit gate:

- session behavior no longer depends on direct thread locks or shared runtime ownership

### Phase 4: Job Recovery, Label, Trace, Parent/Child Semantics

Files:

- `crates/argus-job/src/job_manager.rs`
- `crates/argus-job/src/thread_pool.rs` if any compatibility residue remains

Plan:

- move thread label lookup, trace-path access, runtime status inspection, and parent-child runtime interactions to owner-backed queries
- keep lineage authority in persisted metadata and owner-side recovery
- explicitly avoid using pool summaries or generic handles as authority for parent/root session/job metadata

Exit gate:

- job flows no longer require shared runtime ownership to resolve labels, memory, or trace paths

### Phase 5: Boundary Cleanup And Residue Removal

Files:

- `crates/argus-wing/src/lib.rs`
- workspace-wide residue scans

Plan:

- adapt composition-root and facade tests to the new runtime contract
- remove remaining import/type aliases that expose the old shared-lock model
- verify no stale compatibility helpers recreate the old model under a new name

Exit gate:

- cross-crate runtime contract is uniformly owner/observer, not mixed

## Migration Seams Requiring Explicit Treatment

### Bootstrap / Hydration

- `build_thread_config`
- `hydrate_turn_log_state`
- `recover_and_validate_metadata`
- any owner bootstrap path that currently assumes direct `Thread` access

Rule:

- bootstrap and hydration stay owner-side, adjacent to `argus-agent`, not pool summaries or generic handle caches

### Pool Idle / Memory Hooks

- runtime idle observer
- runtime shutdown flow
- `estimate_thread_memory`
- any cooling/eviction logic that currently dereferences the thread directly

Rule:

- hooks must target owner-backed query surfaces; the pool is not allowed to inspect `Thread` through shared locks

### Session Cache / List / Broadcast / History / Snapshot

- loaded-thread cache
- session list and active-thread views
- subscription/event fanout
- message history reads
- thread snapshots and summaries

Rule:

- session APIs may use eventual-consistency snapshots, but authorization and lineage checks must still defer to authoritative metadata when required

### No-Lock Test Replacements

- agent runtime tests
- pool tests
- session tests
- wing compatibility tests

Rule:

- tests must model runtime ownership with the same handle/query contract used in production; no “tests still use shared locks” exception

## RALPLAN-DR Summary

### Principles

1. Fix runtime ownership, not just names.
2. Keep authority in persisted metadata and owner recovery.
3. Treat migration seams as first-class work, not cleanup.
4. Prove shared-runtime ownership is gone with residue checks and behavior tests.

### Top Drivers

1. The lock-free source task requires a single-owner runtime, not a shared object with nicer wrappers.
2. Current repo state already solved pool extraction and root injection, so the plan must target the remaining ownership defect.
3. Session/job semantics must stay authoritative in owner recovery and persisted metadata, not drift into pool summaries or generic handles.

### Options

- Option A: Wrap `Arc<RwLock<Thread>>` in a new handle and defer true ownership repair.
  - Invalid because it preserves the broken model under a new name.
- Option B: Introduce a real owner/observer split with owner-backed query surfaces and migrate all callers.
  - Chosen because it matches the source task and removes the actual defect.
- Option C: Keep mixed mode where some paths use handles but tests/pool/session still read locks directly.
  - Invalid because it preserves the old semantics in critical seams and fails the acceptance criteria.

## ADR

### Decision

Adopt a true owner/observer runtime contract for `Thread`: `argus-agent` owns the single mutable runtime task, and other crates interact only through explicit handles, mailbox/event surfaces, and owner-backed snapshots/queries.

### Drivers

- The source lock-free spec forbids `Arc<RwLock<Thread>>` in main runtime paths.
- Architect flagged handle underspecification and metadata authority drift.
- Critic required explicit migration seams and stronger verification around shared-runtime residue.
- Brownfield repo facts already show crate extraction and root injection are complete; the remaining gap is runtime ownership.

### Alternatives Considered

- Rename the shared object and keep the same storage model.
- Keep pool/session/job mixed mode while only changing `argus-agent`.
- Move authority into pool summaries or generic runtime handles for convenience.

### Why Chosen

It is the only approach that removes the shared-runtime illusion, preserves authority boundaries, and gives pool/session/job explicit read surfaces without reintroducing ownership leakage.

### Consequences

- `argus-agent` becomes the sole home of mutable thread runtime state.
- `argus-thread-pool` becomes a resident-runtime registry over handles and summaries, not thread objects.
- `argus-session` and `argus-job` must switch from direct thread access to explicit query/snapshot APIs.
- Some read paths become explicitly eventually consistent.
- Verification must include residue scans for renamed shared-object wrappers.

### Follow-ups

- Audit protocol-facing summary types after the runtime contract lands.
- Decide whether any compatibility summary fields can be simplified in a later pass.
- Keep future changes from making session/job metadata authoritative in pool caches.

## Execution And Staffing Guidance

### Available Agent Types

- `planner`
- `architect`
- `critic`
- `executor`
- `debugger`
- `test-engineer`
- `verifier`
- `explore`
- `code-reviewer`
- `security-reviewer`
- `writer`

### Recommended Ralph Path

- Use `ralph` when one owner should drive the sequence end-to-end after plan approval.
- Ralph lane order:
  1. `argus-agent` owner/handle core
  2. `argus-thread-pool` residency + hooks
  3. `argus-session` cache/list/history/snapshot
  4. `argus-job` label/trace/parent-child reads
  5. workspace residue cleanup + verification

### Recommended Team Staffing

- Lane 1: `architect` or `executor`
  - Ownership: `crates/argus-agent/src/thread.rs`, `thread_bootstrap.rs`
  - Reasoning: `high`
- Lane 2: `executor`
  - Ownership: `crates/argus-thread-pool/src/lib.rs`
  - Reasoning: `high`
- Lane 3: `executor`
  - Ownership: `crates/argus-session/src/manager.rs`, `session.rs`
  - Reasoning: `high`
- Lane 4: `executor` or `debugger`
  - Ownership: `crates/argus-job/src/job_manager.rs`
  - Reasoning: `high`
- Lane 5: `test-engineer`
  - Ownership: regression tests, residue checks, command matrix
  - Reasoning: `medium`
- Sidecar: `verifier`
  - Ownership: acceptance evidence, grep residue proof, final validation
  - Reasoning: `high`

### Launch Hints

- Start with `argus-agent` before touching session/job callers; otherwise callers will invent local wrappers.
- Do not run `argus-thread-pool` and `argus-session` edits in parallel until the handle/query contract is explicit.
- Allow `test-engineer` to prepare residue checks and no-lock test targets in parallel with implementation once the contract is fixed.
- If using team mode, give each lane a disjoint write scope and forbid fallback wrappers around `Arc<RwLock<Thread>>`.

### Team Verification Path

1. `verifier` confirms the owner/handle contract exists and no wrapper aliases preserve shared ownership.
2. `test-engineer` runs crate tests and explicit residue scans.
3. `verifier` checks authority boundaries:
   - parent/child/session/job relationships still resolve from persisted metadata or owner-side recovery
   - pool summaries, `thread_sessions`, and generic handles are cache-only
4. `code-reviewer` performs a final regression pass on migration seams:
   - bootstrap/hydration
   - idle/memory hooks
   - session list/broadcast/history/snapshot
   - no-lock tests

## Verification Plan

### Residue Checks

- `rg -n "Arc<RwLock<Thread>>|Weak<RwLock<Thread>>" crates/argus-agent crates/argus-thread-pool crates/argus-session crates/argus-job`
- `rg -n "spawn_reactor\\(thread: Arc<RwLock<Self>>\\)|spawn_reactor\\(.*Arc<RwLock<Self>>" crates/argus-agent`
- `rg -n "type .*=\s*Arc<RwLock<Thread>>|type .*=\s*Weak<RwLock<Thread>>|:\s*Arc<RwLock<Thread>>|:\s*Weak<RwLock<Thread>>" crates`
- `rg -n "inner:\s*Arc<RwLock<Thread>>|inner:\s*Weak<RwLock<Thread>>|thread:\s*Arc<RwLock<Thread>>|thread:\s*Weak<RwLock<Thread>>" crates`

### Acceptance Checks

- label lookup succeeds through handle/query surfaces without `thread.read().await`
- memory estimation succeeds through owner-backed query surfaces without shared thread access
- trace-path reads succeed through owner-backed query or recovery paths without shared thread access
- bootstrap, hydration, and recovery succeed through owner-side seams without reintroducing shared runtime ownership
- session/job/pool behavior no longer depends on shared runtime ownership

### Mandatory Command Set

1. `cargo test -p argus-agent`
2. `cargo test -p argus-thread-pool`
3. `cargo test -p argus-session`
4. `cargo test -p argus-job`
5. `cargo test -p argus-wing`
6. `cargo test`
