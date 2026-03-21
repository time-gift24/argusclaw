# argus-session — 会话与线程聚合层

## 职责

在 Thread 之上增加一层聚合：Session 管理多个 Thread，负责从数据库加载/保存、解决 provider 依赖、以及协调各子系统的初始化。

```
argus-wing / cli
        │
        ▼
  SessionManager
        │
        ├── provider_resolver    ← 解析 LLM provider（打破循环依赖）
        ├── template_manager     ← 获取 AgentRecord（system prompt 等）
        ├── tool_manager        ← 全局工具注册表
        ├── compactor_manager   ← 上下文压缩策略
        ├── pool (SqlitePool)   ← 持久化存储
        │
        ▼
   Session (内存缓存)
        │
        └── threads: DashMap<ThreadId, Arc<Mutex<Thread>>>
                │
                ▼
             Thread
                │
                └── Turn (每条消息创建，用完即销毁)
```

## 核心抽象

### Session

Session 是 Thread 的内存容器。它本身**不执行任何逻辑**，只提供：
- `add_thread()` / `remove_thread()` / `get_thread()` — 线程的增删查
- `list_threads()` — 返回 ThreadSummary 列表（从所有线程聚合）
- `thread_ids()` — 获取所有线程 ID

每个 Thread 用 `Arc<Mutex<Thread>>` 包装：`Arc` 允许多个引用（同时订阅事件 + 发送消息），`Mutex` 保证同一时刻只有一个写操作。

### SessionManager

SessionManager 是入口点，负责：

**1. 两层存储**
- 内存：`DashMap<SessionId, Arc<Session>>` — 活跃会话缓存
- 持久化：SQLite — sessions 表 + threads 表

**2. 惰性加载**（`load()`）
```
请求 session
  │ sessions.contains(id)?
  │   ├─ 是 → 返回缓存
  │   └─ 否 → 从 DB 加载
  │         ├─ 查询 sessions 表 → Session(id, name)
  │         ├─ 查询 threads 表 → ThreadMetadata[]
  │         │   ├─ 解析 provider_id → provider_resolver.resolve()
  │         │   ├─ 解析 template_id → template_manager.get()
  │         │   ├─ 创建 ThreadBuilder
  │         │   └─ session.add_thread()
  │         └─ sessions.insert()
  │
  ▼
返回 Arc<Session>
```

加载时遇到的错误（provider 解析失败、template 不存在、Thread 构建失败）被记录为 warning，单个线程跳过不影响其他线程。

**3. Provider 解析**
ProviderResolver 是一个 trait（打破循环依赖）：
```rust
trait ProviderResolver {
    async fn resolve(&self, id: ProviderId) -> Result<Arc<dyn LlmProvider>>;
    async fn default_provider(&self) -> Result<Arc<dyn LlmProvider>>;
}
```
argus-wing 实现这个 trait，argus-session 只依赖接口。

**4. Provider 选择优先级**
创建 Thread 时 provider 的选择顺序：
```
explicit_provider_id（调用方指定）
  > agent_record.provider_id（模板中配置）
  > default_provider（ProviderResolver 返回）
```

**5. Turn 追踪配置**
每个 Thread 在加载/创建时自动启用 trace：
```rust
TraceConfig::new(true, trace_dir.join(thread_id.inner().to_string()))
```
trace 文件写入 `{trace_dir}/{thread_id}/{turn_number}.json`。

### 数据库Schema（概念）

```sql
sessions (id, name, created_at, updated_at)
threads  (id, session_id, template_id, provider_id, title, token_count, turn_count, created_at, updated_at)
  -- threads.session_id REFERENCES sessions(id) ON DELETE CASCADE
```

SessionManager 不创建 schema（由 argus-repository 或迁移工具负责）。

## 公共 API

```rust
// 会话级别
manager.list_sessions()           // 列出所有会话（DB 查询）
manager.create(name)               // 创建会话
manager.delete(session_id)         // 删除会话（含 cascade 删除 threads）
manager.load(session_id)           // 加载会话到内存

// 线程级别
manager.create_thread(session_id, template_id, explicit_provider_id?)
manager.delete_thread(session_id, thread_id)
manager.list_threads(session_id)    // 内存中有则从内存返回，否则查 DB
manager.send_message(session_id, thread_id, message) // 加载 + 发送 + 忽略结果

// 事件订阅
manager.subscribe(session_id, thread_id)  // 返回 broadcast::Receiver<ThreadEvent>
```

## 约束

- **SessionManager 依赖 SQLite**：通过 `SqlitePool`。Pool 的生命周期由调用方管理（argus-wing 或测试）。
- **Schema 不在 SessionManager 内管理**：假设 schema 已存在。SessionManager 直接执行 SQL。
- **Thread 消息历史不自动持久化**：SessionManager.load() 从 DB 加载 Thread 元数据（title、token_count、turn_count），但不还原消息历史。消息历史在内存中，重启后丢失。
- **没有乐观并发控制**：多个 writer 同时修改 Thread 状态（通过不同的 Mutex）没有冲突检测。
- **unload() 只是从 DashMap 移除**：不写回 DB。内存中的 Session 和 Thread 被 drop。

## 扩展点

**替换持久化层**：当前硬编码 SQLite。如果要支持其他数据库，修改 `manager.rs` 中所有 `sqlx::query` 调用，抽象出 `SessionRepository` trait。

**添加 Session 级别的事件**：当前只有 ThreadEvent。如果需要 Session 级别事件（会话被删除、会话中的线程数变化），在 `Session` 中增加 `broadcast::Sender<SessionEvent>`。

## 下游依赖

```
argus-wing  — 实现 ProviderResolver，提供 LLMManager
cli         — 直接使用 SessionManager 暴露的 API
```
