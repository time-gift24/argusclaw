# Test Spec: Lock-Free Thread Owner/Observer Redesign

## Metadata

- Date: 2026-04-20
- Source specs:
  - `.omx/specs/deep-interview-argus-agent-thread-core-lock-free.md`
  - `.omx/specs/deep-interview-session-threadpool-job-boundaries.md`
- Companion PRD: `.omx/plans/prd-thread-owner-lock-free-refactor.md`

## Goal

Prove that the current brownfield workspace keeps its existing `argus-thread-pool` crate and composition-root pool injection while removing shared runtime ownership from the thread model.

## Re-anchored Repo State

The test plan assumes these facts are already true:

- `argus-thread-pool` already exists in the workspace.
- `ArgusWing` already constructs the shared pool and injects it into both managers.
- `argus-session` already depends on `argus-thread-pool`.
- `SessionManager` already maintains a `thread_sessions` adapter/index.

The tests target the remaining defect:

- shared runtime ownership still appears as `Arc<RwLock<Thread>>`, `Weak<RwLock<Thread>>`, and `spawn_reactor(Arc<RwLock<Self>>)`

## Contract Under Test

### Runtime Ownership

- `Thread` runs under one mutable owner task
- observers use explicit handles only
- no handle is allowed to wrap or alias `Arc<RwLock<Thread>>`

### Read Semantics

- mailbox, event, snapshot, summary, label, memory-estimate, and trace-path reads may be eventually consistent
- relationship authority stays in persisted metadata and owner-side recovery
- pool summaries, session indexes, and generic handles are caches/adapters only

## Test Matrix

### 1. `argus-agent` Owner/Handle Runtime Tests

Files:

- `crates/argus-agent/src/thread.rs`
- `crates/argus-agent/src/thread_bootstrap.rs`
- `crates/argus-agent/tests/trace_integration_test.rs`

Must prove:

- the reactor no longer starts from `spawn_reactor(Arc<RwLock<Self>>)`
- message ingress, interrupt, shutdown, and turn settlement work through the new owner/handle contract
- event subscription still works without direct thread lock access
- bootstrap, hydration, and metadata recovery stay owner-side and do not require shared runtime ownership
- trace integration tests no longer model the runtime as `Arc<RwLock<Thread>>`

### 2. `argus-thread-pool` Residency And Hook Tests

Files:

- `crates/argus-thread-pool/src/lib.rs`

Must prove:

- runtime registration, admission, cooling, eviction, and shutdown operate on handles/summaries
- idle observer and memory-estimation hooks use query surfaces instead of `thread.read().await`
- pool summaries remain cache/read models only
- `loaded_runtime` or replacement surfaces do not return shared runtime ownership

### 3. `argus-session` Cache / List / Broadcast / History / Snapshot Tests

Files:

- `crates/argus-session/src/manager.rs`
- `crates/argus-session/src/session.rs`

Must prove:

- session cache no longer uses `Weak<RwLock<Thread>>`
- send/cancel/list/activate/subscribe/history/snapshot flows work through handles and snapshots
- `thread_sessions` remains an adapter/index, not the authority for lineage
- owner-backed label lookup still supports scheduler and mailbox-facing UX

### 4. `argus-job` Label / Trace / Parent-Child Tests

Files:

- `crates/argus-job/src/job_manager.rs`

Must prove:

- label lookup no longer depends on shared thread reads
- trace-path access no longer depends on pool-returned `Arc<RwLock<Thread>>`
- parent/child and root-session lineage still resolve from persisted metadata or owner recovery
- job runtime summary is not treated as relationship authority

### 5. `argus-wing` Boundary / Compatibility Tests

Files:

- `crates/argus-wing/src/lib.rs`

Must prove:

- facade behavior still works with the new runtime contract
- any summary compatibility fields still come from adapter/query layers, not shared runtime ownership
- session access checks remain correct even though reads are eventually consistent

## Required Residue Checks

These are required exit checks, not optional greps:

1. `rg -n "Arc<RwLock<Thread>>|Weak<RwLock<Thread>>" crates/argus-agent crates/argus-thread-pool crates/argus-session crates/argus-job`
2. `rg -n "spawn_reactor\\(thread: Arc<RwLock<Self>>\\)|spawn_reactor\\(.*Arc<RwLock<Self>>" crates/argus-agent`
3. `rg -n "thread\\.read\\(\\)\\.await|thread\\.write\\(\\)\\.await" crates/argus-thread-pool crates/argus-session crates/argus-job`
4. `rg -n "type .*=\s*Arc<RwLock<Thread>>|type .*=\s*Weak<RwLock<Thread>>|:\s*Arc<RwLock<Thread>>|:\s*Weak<RwLock<Thread>>" crates`
5. `rg -n "inner:\s*Arc<RwLock<Thread>>|inner:\s*Weak<RwLock<Thread>>|thread:\s*Arc<RwLock<Thread>>|thread:\s*Weak<RwLock<Thread>>" crates`

Failure on any residue check blocks completion.

## Required Behavioral Checks

1. Label lookup reads succeed through owner-backed query surfaces in session/job paths.
2. Memory estimation still feeds pool summaries without direct shared-thread reads.
3. Trace-path reads succeed for loaded and recoverable runtimes without shared runtime ownership.
4. Bootstrap, hydration, and metadata recovery succeed through owner-side seams without shared runtime ownership.
5. Pool lifecycle transitions still emit expected runtime state and summary updates.
6. Session send/cancel/list/history/snapshot/subscribe behavior still works under eventual-consistency reads.
7. Job dispatch/cancel/complete flows still work without shared runtime ownership.
8. Session/job/pool tests demonstrate that runtime ownership is not shared across crate boundaries.

## Required Test Cases

1. Owner runtime happy path: create runtime, send message, settle turn, read snapshot.
2. Owner runtime interrupt path: start turn, interrupt, verify terminal state and snapshot update.
3. Owner runtime shutdown path: handle-triggered shutdown without lock-based access.
4. Owner bootstrap/hydration regression for `build_thread_config`, `hydrate_turn_log_state`, and metadata validation without shared runtime ownership.
5. Pool registration/admission/cooling/eviction regression on handles.
6. Pool idle observer regression proving no direct thread reads remain.
7. Pool memory-estimation regression proving owner-backed query path.
8. Session cache replacement regression proving no `Weak<RwLock<Thread>>` remains.
9. Session list + snapshot regression under eventual consistency.
10. Session broadcast/subscribe regression through handle/event surfaces.
11. Session history regression through owner-backed snapshot/query path.
12. Session authorization regression proving `thread_sessions` remains adapter-only.
13. Job label lookup regression through owner-backed query path.
14. Job trace-path regression for loaded runtime and recovered runtime.
15. Job parent/child lineage regression proving persisted metadata remains authoritative.
16. Wing compatibility regression for thread pool state / session-bound access checks.
17. No-lock test replacement regression for former `Arc<RwLock<Thread>>` test helpers.

## Phase-Gated Verification

### Gate 1: `argus-agent`

- reactor entrypoint changed
- bootstrap/hydration/recovery seams are covered explicitly
- integration tests no longer use shared runtime locks
- owner/handle/query semantics proven locally

### Gate 2: `argus-thread-pool`

- handle-based residency works
- idle/memory hooks are query-based
- no shared runtime object escapes the pool

### Gate 3: `argus-session`

- cache/list/broadcast/history/snapshot flows pass on the new contract
- `thread_sessions` remains cache/adapter only

### Gate 4: `argus-job`

- label and trace-path reads pass without shared ownership
- lineage authority remains persisted metadata / owner recovery

### Gate 5: Workspace

- residue checks are clean
- all crate tests and full workspace tests pass

## Concise RALPLAN-DR Summary

### Principles

1. Test the real ownership change, not a renamed wrapper.
2. Verify authority boundaries separately from cache behavior.
3. Make migration seams first-class test surfaces.

### Drivers

1. The lock-free task forbids shared runtime ownership in production and tests.
2. Brownfield repo state already solved crate extraction and root wiring.
3. Reviews required stronger proof around metadata authority and residue removal.

### Option Chosen

- Verify a true owner/observer runtime contract end to end.
- Reject partial verification that only proves APIs were renamed.

## ADR

### Decision

Treat residue checks, authority-boundary tests, and migration-seam regressions as mandatory completion evidence for this redesign.

### Why

Passing behavior tests alone is insufficient because a renamed `Arc<RwLock<Thread>>` wrapper could still satisfy many flows while violating the source task.

### Consequences

- verification includes grep-style residue checks plus behavior tests
- session/job authority checks are distinct from pool cache checks
- no-lock test replacements are required, not cleanup

## Agent Roster And Execution Guidance

### Available Agent Types

- `planner`
- `architect`
- `executor`
- `debugger`
- `test-engineer`
- `verifier`
- `code-reviewer`
- `explore`

### Reasoning By Lane

- owner contract lane: `high`
- pool lane: `high`
- session lane: `high`
- job lane: `high`
- test lane: `medium`
- verification lane: `high`

### Ralph / Team Guidance

- `ralph` is appropriate if one owner implements and verifies gates in order.
- `team` is appropriate only after the owner/handle contract is fixed enough that pool/session/job lanes can target stable interfaces.
- Do not split session and job lanes before the query/snapshot contract exists.

### Launch Hints

- Start the verification harness with residue checks already defined so wrapper regressions are caught early.
- Keep one dedicated `test-engineer` lane responsible for no-lock test replacements and final command execution.
- Reserve `verifier` for authority-boundary proof, not just command pass/fail.

### Team Verification Path

1. `test-engineer` runs phase-gated crate tests.
2. `verifier` runs residue checks and authority-boundary review.
3. `code-reviewer` audits migration seams and confirms no renamed shared-object wrapper survived.
4. Final handoff requires both behavior evidence and clean residue output.

## Verification Commands

1. `cargo test -p argus-agent`
2. `cargo test -p argus-thread-pool`
3. `cargo test -p argus-session`
4. `cargo test -p argus-job`
5. `cargo test -p argus-wing`
6. `cargo test`
7. `rg -n "Arc<RwLock<Thread>>|Weak<RwLock<Thread>>" crates/argus-agent crates/argus-thread-pool crates/argus-session crates/argus-job`
8. `rg -n "spawn_reactor\\(thread: Arc<RwLock<Self>>\\)|spawn_reactor\\(.*Arc<RwLock<Self>>" crates/argus-agent`
9. `rg -n "thread\\.read\\(\\)\\.await|thread\\.write\\(\\)\\.await" crates/argus-thread-pool crates/argus-session crates/argus-job`
