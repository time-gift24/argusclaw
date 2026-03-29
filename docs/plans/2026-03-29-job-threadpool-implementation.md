# Job ThreadPool Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 把 job 的执行单位升级为真实 thread，引入可监控内存和用量的 `ThreadPool`，并在 desktop 中提供独立的 Thread 监控页签。

**Architecture:** 在 `argus-job` 中新增 `ThreadPool` 作为 job-thread 的唯一执行协调器，`JobManager` 只保留 job 语义和状态推进。协议层补充 pool 快照与事件，`ArgusWing`/Tauri 暴露查询与订阅接口，desktop 在现有 chat/store 基础上增加 Thread Monitor 视图。

**Tech Stack:** Rust (`tokio`, `sqlx`, `serde`, `broadcast`), Tauri v2, Next.js/React, Zustand, workspace cargo tests

---

### Task 1: 扩展协议契约，定义 ThreadPool 快照和事件

**Files:**
- Modify: `crates/argus-protocol/src/events.rs`
- Modify: `crates/argus-protocol/src/lib.rs`
- Test: `crates/argus-protocol/src/events.rs`

**Step 1: 为协议新增失败测试和序列化测试**

在 `crates/argus-protocol/src/events.rs` 的测试模块新增：

```rust
#[test]
fn thread_pool_snapshot_round_trips_through_json() {
    let snapshot = ThreadPoolSnapshot {
        max_threads: 8,
        active_threads: 2,
        queued_jobs: 1,
        running_threads: 1,
        cooling_threads: 1,
        evicted_threads: 3,
        estimated_memory_bytes: 4096,
        peak_estimated_memory_bytes: 8192,
        process_memory_bytes: Some(16_384),
        peak_process_memory_bytes: Some(32_768),
        resident_thread_count: 2,
        avg_thread_memory_bytes: 2048,
        captured_at: "2026-03-29T00:00:00Z".to_string(),
    };

    let value = serde_json::to_value(&snapshot).unwrap();
    let restored: ThreadPoolSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(restored.max_threads, 8);
    assert_eq!(restored.queued_jobs, 1);
}
```

**Step 2: 运行测试并确认失败**

Run: `cargo test -p argus-protocol thread_pool_snapshot_round_trips_through_json -- --exact`

Expected: FAIL，因为 `ThreadPoolSnapshot` 和相关事件还不存在。

**Step 3: 增加协议类型**

在 `crates/argus-protocol/src/events.rs` 增加：

- `ThreadRuntimeSnapshot`
- `ThreadPoolSnapshot`
- `ThreadPoolEventReason`
- `ThreadEvent` 新变体：
  - `ThreadBoundToJob`
  - `ThreadPoolQueued`
  - `ThreadPoolStarted`
  - `ThreadPoolCooling`
  - `ThreadPoolEvicted`
  - `ThreadPoolMetricsUpdated`

并在 `crates/argus-protocol/src/lib.rs` 导出这些类型。

**Step 4: 重新运行协议测试**

Run: `cargo test -p argus-protocol`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-protocol/src/events.rs crates/argus-protocol/src/lib.rs
git commit -m "feat: add thread pool protocol events"
```

### Task 2: 新建 ThreadPool 核心，并把 JobManager 改成提交调度意图

**Files:**
- Create: `crates/argus-job/src/thread_pool.rs`
- Modify: `crates/argus-job/src/lib.rs`
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/dispatch_tool.rs`
- Modify: `crates/argus-job/src/types.rs`
- Test: `crates/argus-job/src/thread_pool.rs`
- Test: `crates/argus-job/src/job_manager.rs`

**Step 1: 为 ThreadPool 状态机写失败测试**

在新建的 `crates/argus-job/src/thread_pool.rs` 中先写测试，覆盖：

- `enqueue_job` 创建 job-thread 绑定
- `collect_metrics` 统计 queued/running/cooling
- cooling 后 evict

示例骨架：

```rust
#[tokio::test]
async fn enqueue_job_creates_binding_and_updates_metrics() {
    let pool = test_thread_pool();

    let thread_id = pool
        .enqueue_job(test_request("job-1"))
        .await
        .expect("enqueue should succeed");

    let snapshot = pool.collect_metrics().await;
    assert_eq!(snapshot.queued_jobs, 1);
    assert_eq!(pool.get_thread_binding("job-1"), Some(thread_id));
}
```

**Step 2: 运行测试并确认失败**

Run: `cargo test -p argus-job enqueue_job_creates_binding_and_updates_metrics -- --exact`

Expected: FAIL，因为 `ThreadPool` 还不存在。

**Step 3: 实现最小 ThreadPool**

在 `crates/argus-job/src/thread_pool.rs` 实现：

- `ThreadPool`
- 活跃 thread 注册表
- `enqueue_job`
- `get_thread_binding`
- `collect_metrics`
- `mark_running` / `mark_cooling` / `evict_if_idle` 之类的内部辅助方法

先用内存态实现，不在这一步追求完整前端桥接。

**Step 4: 把 JobManager 改成依赖 ThreadPool**

在 `crates/argus-job/src/job_manager.rs`：

- 删除 `spawn_job_executor` 直接构建 `TurnBuilder` 的路径
- 保留 job 状态、结果跟踪和 `get_job_result_status`
- 新增类似 `dispatch_job` / `enqueue_job` 的入口，让 `ThreadPool` 接管 thread 创建和执行

在 `crates/argus-job/src/dispatch_tool.rs`：

- `dispatch_job` 仍生成 `job_id`
- 不再生成 `thread_id`
- 调用 `JobManager` 的新接口

**Step 5: 运行 argus-job 测试**

Run: `cargo test -p argus-job`

Expected: PASS

**Step 6: Commit**

```bash
git add crates/argus-job/src/thread_pool.rs crates/argus-job/src/lib.rs crates/argus-job/src/job_manager.rs crates/argus-job/src/dispatch_tool.rs crates/argus-job/src/types.rs
git commit -m "feat: add thread pool job orchestration"
```

### Task 3: 接通持久化和 ArgusWing 注入，让 job-thread 真正可恢复

**Files:**
- Modify: `crates/argus-repository/src/traits/job.rs`
- Modify: `crates/argus-repository/src/sqlite/job.rs`
- Modify: `crates/argus-repository/src/traits/thread.rs`
- Modify: `crates/argus-repository/src/sqlite/thread.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-wing/src/lib.rs`
- Test: `crates/argus-session/src/manager.rs`
- Test: `crates/argus-wing/src/lib.rs`

**Step 1: 为 job-thread 绑定恢复写失败测试**

在 `crates/argus-wing/src/lib.rs` 或相关集成测试中新增：

```rust
#[tokio::test]
async fn dispatch_job_binds_real_thread_id_and_keeps_it_recoverable() {
    let wing = test_wing();

    let job = wing.enqueue_test_job().await;
    let binding = wing
        .thread_pool_snapshot()
        .await
        .threads
        .iter()
        .find(|t| t.job_id.as_deref() == Some(job.id.as_str()));

    assert!(binding.is_some());
}
```

**Step 2: 运行测试并确认失败**

Run: `cargo test -p argus-wing dispatch_job_binds_real_thread_id_and_keeps_it_recoverable -- --exact`

Expected: FAIL，因为 `ThreadPool` 还没有通过 `ArgusWing` 暴露。

**Step 3: 接入 repository 和恢复路径**

按最小方案实现：

- 继续使用 `jobs.thread_id` 作为 job 的执行 thread 绑定字段
- 允许 job-thread 的 `ThreadRecord.session_id` 为空，避免污染普通 session 列表
- 在 `ThreadPool` 中通过 `ThreadRepository::upsert_thread`、`get_thread`、`update_thread_stats` 保持恢复所需最小持久化状态
- 在 `ArgusWing` 中注入 `ThreadPool`，暴露查询入口

如发现 `ThreadPool` 需要额外的 repository trait，再小步补充，不要先做大迁移。

**Step 4: 运行受影响测试**

Run: `cargo test -p argus-session`

Run: `cargo test -p argus-wing`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-repository/src/traits/job.rs crates/argus-repository/src/sqlite/job.rs crates/argus-repository/src/traits/thread.rs crates/argus-repository/src/sqlite/thread.rs crates/argus-session/src/manager.rs crates/argus-wing/src/lib.rs
git commit -m "feat: persist and expose job thread bindings"
```

### Task 4: 补 Tauri 命令和事件桥接，把 ThreadPool 观测数据送到前端

**Files:**
- Modify: `crates/desktop/src-tauri/src/events/thread.rs`
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Modify: `crates/desktop/src-tauri/src/lib.rs`
- Modify: `crates/desktop/lib/types/chat.ts`
- Modify: `crates/desktop/lib/tauri.ts`
- Test: `crates/desktop/src-tauri/src/events/thread.rs`
- Test: `crates/desktop/tests/chat-tauri-bindings.test.mjs`

**Step 1: 为前后端绑定写失败测试**

新增或扩展测试，验证：

- `ThreadEventEnvelope::from_thread_event` 能桥接新的 pool 事件
- `lib/tauri.ts` 暴露 `getThreadPoolSnapshot` 或等价调用

示例：

```rust
#[test]
fn metrics_updated_event_converts_to_frontend_payload() {
    let envelope = ThreadEventEnvelope::from_thread_event(
        "session-1".to_string(),
        ThreadEvent::ThreadPoolMetricsUpdated { snapshot: sample_snapshot() },
    )
    .expect("event should convert");

    assert_eq!(envelope.payload.kind(), "thread_pool_metrics_updated");
}
```

**Step 2: 运行测试并确认失败**

Run: `cargo test -p desktop metrics_updated_event_converts_to_frontend_payload -- --exact`

Run: `pnpm --dir crates/desktop test -- --run chat-tauri-bindings.test.mjs`

Expected: FAIL

**Step 3: 增加桥接代码**

- 在 `crates/desktop/src-tauri/src/events/thread.rs` 增加新的 payload 变体
- 在 `crates/desktop/src-tauri/src/commands.rs` 新增查询 `ThreadPoolSnapshot` 的命令
- 在 `crates/desktop/src-tauri/src/lib.rs` 注册命令
- 在 `crates/desktop/lib/types/chat.ts` 和 `crates/desktop/lib/tauri.ts` 加对应 TS 类型和 invoke 封装

**Step 4: 重新运行测试**

Run: `cargo test -p desktop`

Run: `pnpm --dir crates/desktop test -- --run chat-tauri-bindings.test.mjs`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/desktop/src-tauri/src/events/thread.rs crates/desktop/src-tauri/src/commands.rs crates/desktop/src-tauri/src/lib.rs crates/desktop/lib/types/chat.ts crates/desktop/lib/tauri.ts crates/desktop/tests/chat-tauri-bindings.test.mjs
git commit -m "feat: expose thread pool telemetry to desktop"
```

### Task 5: 在 desktop 中加入 Thread Monitor 页签和状态存储

**Files:**
- Modify: `crates/desktop/components/chat/chat-screen.tsx`
- Modify: `crates/desktop/lib/chat-store.ts`
- Create: `crates/desktop/components/thread-monitor/thread-monitor-screen.tsx`
- Create: `crates/desktop/components/thread-monitor/thread-monitor-table.tsx`
- Create: `crates/desktop/components/thread-monitor/thread-monitor-summary.tsx`
- Test: `crates/desktop/tests/chat-store-session-model.test.mjs`
- Test: `crates/desktop/tests/chat-page-runtime-integration.test.mjs`

**Step 1: 先写 store 和 UI 的失败测试**

覆盖：

- store 能保存 `threadPoolSnapshot` 和 thread monitor 列表
- 首页能出现 `Chat` / `Threads` 两个页签
- Thread Monitor 页签能展示 pool 总览卡片和线程表格

示例断言：

```js
assert.match(storeSource, /threadPoolSnapshot:/);
assert.match(pageSource, /TabsTrigger value="threads"/);
assert.match(pageSource, /estimated_memory_bytes/);
```

**Step 2: 运行测试并确认失败**

Run: `pnpm --dir crates/desktop test -- --run chat-store-session-model.test.mjs chat-page-runtime-integration.test.mjs`

Expected: FAIL

**Step 3: 增加 Zustand 状态与只读监控 UI**

在 `crates/desktop/lib/chat-store.ts`：

- 增加 `threadPoolSnapshot`
- 增加刷新和事件处理逻辑
- 保持聊天状态与监控状态分离

在 `crates/desktop/components/chat/chat-screen.tsx`：

- 用现有 `Tabs` 组件包裹 `Chat` 和 `Threads`

新增 `crates/desktop/components/thread-monitor/*`：

- 总览卡片组件
- 线程表格组件
- 详情/空态占位

第一版只做只读视图，不加控制按钮。

**Step 4: 重新运行前端测试**

Run: `pnpm --dir crates/desktop test -- --run chat-store-session-model.test.mjs chat-page-runtime-integration.test.mjs`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/desktop/components/chat/chat-screen.tsx crates/desktop/lib/chat-store.ts crates/desktop/components/thread-monitor/thread-monitor-screen.tsx crates/desktop/components/thread-monitor/thread-monitor-table.tsx crates/desktop/components/thread-monitor/thread-monitor-summary.tsx crates/desktop/tests/chat-store-session-model.test.mjs crates/desktop/tests/chat-page-runtime-integration.test.mjs
git commit -m "feat: add thread monitor desktop tab"
```

### Task 6: 收尾验证，补全回收/恢复测试并跑全量基线

**Files:**
- Modify: `crates/argus-job/src/thread_pool.rs`
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/desktop/components/thread-monitor/thread-monitor-screen.tsx`
- Test: `crates/argus-job/src/thread_pool.rs`
- Test: `crates/desktop/tests/chat-page-runtime-integration.test.mjs`

**Step 1: 增加恢复与回收的失败测试**

补上：

- evicted thread 再次被 job 激活
- 监控快照在 cooling/evicted 之间正确切换
- 前端能展示 cooling / evicted 状态

**Step 2: 运行测试并确认失败**

Run: `cargo test -p argus-job`

Run: `pnpm --dir crates/desktop test -- --run chat-page-runtime-integration.test.mjs`

Expected: FAIL

**Step 3: 实现恢复与收尾**

- 补全 `ThreadPool` 恢复路径
- 清理重复状态推进代码
- 确保 metrics 采样失败只降级，不阻塞执行

**Step 4: 跑完整验证**

Run: `cargo fmt --all`

Run: `cargo test`

Run: `pnpm --dir crates/desktop test`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-job/src/thread_pool.rs crates/argus-job/src/job_manager.rs crates/desktop/components/thread-monitor/thread-monitor-screen.tsx crates/desktop/tests/chat-page-runtime-integration.test.mjs
git commit -m "test: cover thread pool recovery flow"
```

### Task 7: 最终检查并准备提审

**Files:**
- Modify: `docs/plans/2026-03-29-job-threadpool-design.md` only if design drift is discovered
- Modify: `docs/plans/2026-03-29-job-threadpool-implementation.md` only if plan needs to reflect actual file changes

**Step 1: 检查工作树和文档一致性**

Run: `git status --short`

Expected: 只有当前任务预期修改，且没有误提交 `.worktrees` 内容。

**Step 2: 复盘验证结果**

Run: `cargo test > /tmp/job-threadpool-cargo-test.log`

Run: `pnpm --dir crates/desktop test > /tmp/job-threadpool-desktop-test.log`

Expected: 两份日志都显示通过。

**Step 3: 更新文档（如有必要）**

仅在实现与设计偏离时更新设计文档或计划文档，避免文档漂移。

**Step 4: 最终提交**

```bash
git add docs/plans/2026-03-29-job-threadpool-design.md docs/plans/2026-03-29-job-threadpool-implementation.md
git commit -m "docs: align thread pool design docs"
```
