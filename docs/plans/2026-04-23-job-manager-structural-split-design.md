# Job Manager Structural Split Design

**Date:** 2026-04-23

## Goal

Split `crates/argus-job/src/job_manager.rs` into smaller internal modules without changing `argus-job`'s public API, runtime behavior, crate boundaries, or overall ownership model.

## Confirmed Constraints

- Keep `JobManager` as the external facade.
- Do not change dispatch / cancel / recovery / runtime semantics.
- Do not move code across crates.
- Do not expand this into a broader architecture rewrite.
- Only do the minimum test movement needed to support the split.

## Current Problem

`job_manager.rs` currently mixes six responsibility clusters in one file:

1. tracked job state and lookup
2. job runtime summary / metrics / lifecycle bridge
3. job-thread binding and recovery
4. execution orchestration
5. persistence helpers
6. mailbox / job-result forwarding

This makes the file hard to navigate and raises the cost of changing one area without reloading unrelated logic.

## Options Considered

### 1. Keep one file, only add comment sections

Pros:
- lowest diff

Cons:
- does not materially improve ownership or navigation
- keeps 3k+ lines of mixed logic in one place

### 2. Split into internal modules, keep `JobManager` facade

Pros:
- matches the requested scope
- improves maintainability without changing public boundaries
- lets tests stay behavior-focused

Cons:
- requires some careful visibility design between modules

### 3. Introduce multiple public managers

Pros:
- strongest ownership separation

Cons:
- exceeds agreed scope
- risks API churn and architecture drift

## Recommended Design

Use option 2.

Create an internal `job_manager/` module tree under `crates/argus-job/src/` and keep `job_manager.rs` as the public facade entrypoint. The facade will continue to define `JobManager` and `JobLookup`, hold shared dependencies, and delegate work to focused private modules.

## Target Module Shape

- `job_manager.rs`
  - public facade
  - shared type definitions that must remain close to `JobManager`
  - internal module declarations / re-exports
- `job_manager/tracking.rs`
  - tracked job store
  - lookup / consume / pruning helpers
- `job_manager/runtime_state.rs`
  - runtime summary store
  - snapshot calculation
  - lifecycle bridge helpers
- `job_manager/binding_recovery.rs`
  - job/thread binding cache helpers
  - parent/child recovery
  - metadata sync
- `job_manager/persistence.rs`
  - job record persistence
  - thread record persistence
  - binding persistence / rollback
- `job_manager/mailbox_result.rs`
  - result forward path
  - delivered mailbox shadow
  - result event broadcast helpers
- `job_manager/execution.rs`
  - dispatch
  - enqueue
  - runtime load/build
  - task delivery
  - turn-result wait loop

## Visibility Rules

- Keep `JobManager` methods grouped by the module that owns them via separate `impl JobManager` blocks in each file.
- Keep storage structs private to the `job_manager` module unless another submodule must construct them.
- Prefer `pub(super)` over broader visibility for helpers shared across submodules.
- Keep test-only helpers local unless reuse clearly reduces duplication.

## Testing Strategy

- Preserve existing `argus-job` behavior tests.
- Move tests only when a module split makes locality materially clearer.
- Prefer keeping integration-style behavior checks in the main `job_manager` test module if they exercise multiple responsibility clusters.

## Risks

- Helper visibility can sprawl if the split is too granular.
- Moving methods across files can accidentally break intra-module assumptions.
- The runtime lifecycle bridge and persistence path are cross-cutting, so they need careful ownership boundaries.

## Mitigations

- Keep the first pass at six modules, not more.
- Preserve existing method names and signatures where possible.
- Run focused `argus-job` tests after the split before considering any cleanup follow-up.
