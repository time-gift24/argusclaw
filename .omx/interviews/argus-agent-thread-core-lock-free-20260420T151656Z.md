# Deep Interview Transcript: argus-agent Thread Core Lock-Free

## Metadata

- Date: 2026-04-20T15:16:56Z
- Profile: standard
- Context type: brownfield
- Final ambiguity: 0.197
- Threshold: 0.20
- Context snapshot: `.omx/context/argus-agent-thread-core-lock-free-20260420T150100Z.md`

## Brownfield facts gathered before questioning

- `crates/argus-agent/src/thread.rs` currently exposes `Thread::spawn_reactor(thread: Arc<RwLock<Self>>)` and uses `Arc<RwLock<Thread>>` to drive runtime mutation.
- `crates/argus-session/src/session.rs` stores `Weak<RwLock<Thread>>`.
- `crates/argus-thread-pool/src/lib.rs` and `crates/argus-job/src/thread_pool.rs` also pass `Arc<RwLock<Thread>>` across crate boundaries.
- In practice, many external callers only take a read lock to reach `send_message`, while true state mutation remains reactor-owned.
- Prior spec `.omx/specs/deep-interview-session-threadpool-job-boundaries.md` already established `argus-agent` as the thread-owned runtime authority.

## Round-by-round transcript

### Round 1

- Target: Intent
- Question: root problem is ownership expression, external mutable access, or lock complexity?
- Answer: `锁本身让模型和实现都变得非常复杂`

### Round 2

- Target: Intent
- Question: ask for a concrete anti-example
- Answer: `这点是我凭感觉提出的，因为明明可以无锁化设计`

### Round 3

- Target: Outcome
- Question: is it enough to hide `Arc<RwLock<Thread>>`, or must the runtime itself also become basically lock-free?
- Answer: `运行时的内部必须基本不再依赖锁 —— 读取信息就直接返回即可`

### Round 4

- Target: Scope
- Question: only refactor `argus-agent`, or also remove `Arc<RwLock<Thread>>` from cross-crate main paths?
- Answer: `2`

### Round 5

- Target: Non-goals
- Question: can tests keep lock-based expression as an exception?
- Answer: `测试里的也删除！`

### Round 6

- Target: Constraints
- Question: are `mpsc` / `broadcast` / `atomic` acceptable, with only `Mutex` / `RwLock` forbidden on the Thread main path?
- Answer: `保留`

### Round 7

- Target: Decision Boundaries
- Question: can OMX decide naming, migration order, handle shape, and cross-crate adaptation details?
- Answer: `不需要`

## Pressure-pass result

The initial position was mostly intuition-based: “it should obviously be lock-free.”  
After pressure, that intuition hardened into an explicit architectural requirement:

- not just hiding the lock from public APIs
- but making the `Thread` runtime itself basically non-lock-based
- while still allowing single-owner-friendly coordination primitives
- and requiring tests to stop legitimizing the old shared-lock model

## Final clarified brief

- `Thread` should be the explicit runtime center in `argus-agent`
- the runtime should converge toward a single-owner model
- only the unique owner should mutate runtime state
- observers do not require strong read consistency
- reads may return direct snapshot/cached values and only need eventual consistency
- `Arc<RwLock<Thread>>` should be removed from the main production path across:
  - `argus-agent`
  - `argus-session`
  - `argus-thread-pool`
  - `argus-job`
- related tests should also stop using the shared-lock model

## Recommended handoff

`$ralplan .omx/specs/deep-interview-argus-agent-thread-core-lock-free.md`
