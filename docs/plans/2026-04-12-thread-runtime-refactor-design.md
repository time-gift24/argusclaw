# ThreadRuntime Refactor Design

**Date:** 2026-04-12  
**Status:** Approved for planning

## Goal

Clarify crate ownership by moving thread runtime management out of `argus-job` and into `argus-agent`, while keeping job dispatch and job lifecycle in `argus-job`.

## Problem

`crates/argus-job/src/thread_pool.rs` currently mixes two different concerns:

- thread runtime lifecycle and recovery
- job dispatch, admission, and binding

That makes `argus-session` depend on `argus-job` for chat-thread runtime operations, even though those concerns align more naturally with `argus-agent`.

## Decision

Introduce a new `argus-agent` runtime owner named `ThreadRuntime`.

`ThreadRuntime` becomes the single home for:

- runtime registration
- runtime subscription
- runtime removal
- runtime summaries and state collection
- thread recovery from trace metadata
- in-memory parent/child runtime caches

`argus-job` keeps:

- dispatch decisions
- queueing and admission control
- job lifecycle state
- job-to-thread binding

The key boundary is:

- `argus-agent` owns what a thread runtime is
- `argus-job` owns when a job should create or bind a thread

## API Shape

`argus-agent` will expose a `ThreadRuntime` type with a thread-centric API:

- `register_thread(...)`
- `subscribe(thread_id)`
- `remove_runtime(thread_id)`
- `runtime_summary(thread_id)`
- `collect_state()`
- `recover_thread(...)`

`register_thread(...)` should accept explicit thread metadata instead of splitting into separate chat/job entry points. The caller provides the thread kind and associated metadata, and `ThreadRuntime` handles registration uniformly.

## Truth Sources

The recent flattening work remains the source of truth:

- parent/child relationships are persisted through directory layout plus `thread.json`
- `parent_thread_by_child` and `child_threads_by_parent` remain runtime caches only

This means the refactor should move those caches if needed, but should not promote them into persisted authority.

## Migration Strategy

Use a staged migration:

1. Add `ThreadRuntime` to `argus-agent` and move thread-runtime-oriented logic behind it.
2. Switch `argus-session` to depend on `ThreadRuntime` for chat runtime operations.
3. Update `argus-job` to compose `ThreadRuntime` and keep only job-specific logic.
4. Remove or rename the remaining `ThreadPool` pieces once the split is complete.

This keeps the behavior stable while making the ownership boundary explicit.

## Risks

- Recovery behavior could regress if runtime caches are treated as durable truth.
- Subscription paths could regress if desktop/session flows still assume `ThreadPool` ownership.
- Job binding can leak back into the runtime layer if the split is not enforced carefully.

## Validation

Preserve and extend tests in three areas:

- `argus-agent`: runtime register/recover/subscribe behavior
- `argus-job`: dispatch and job-to-thread binding behavior
- `argus-session`: chat thread recovery and subscription no longer depend on `argus-job` runtime ownership

## Recommended Next Step

Write an implementation plan that migrates the boundary in small, test-first steps and keeps `job dispatch` inside `argus-job` throughout.
