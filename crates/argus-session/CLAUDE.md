# Argus-Session 会话管理

> 特性：会话管理，协调 log、template、thread、tool 等模块。

## 模块结构

```
src/
├── lib.rs              # 公共 API 导出
├── manager.rs          # SessionManager：会话生命周期管理
├── session.rs          # Session：会话容器（多线程）
└── provider_resolver.rs # ProviderResolver：LLM Provider 解析接口
```

## 核心概念

### 1. Session 结构

**Session** 是多个 Thread 的容器：

```rust
pub struct Session {
    pub id: SessionId,
    pub name: String,
    threads: DashMap<ThreadId, Arc<Mutex<Thread>>>,  // 内存中的线程
}
```

### 2. SessionManager

**SessionManager** 管理会话的持久化和加载：

```rust
pub struct SessionManager {
    pool: SqlitePool,                              // 数据库连接
    sessions: DashMap<SessionId, Arc<Session>>,   // 内存中的会话
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    compactor_manager: Arc<CompactorManager>,
}
```

**关键特性**：
- 惰性加载：会话按需从数据库加载到内存
- 多层存储：内存 + SQLite 持久化
- Thread 聚合管理

### 3. ProviderResolver Trait

```rust
pub trait ProviderResolver: Send + Sync {
    fn resolve(&self, provider_id: &ProviderId) -> Result<Arc<dyn LlmProvider>>;
}
```

## 公共 API

```rust
use argus_session::{SessionManager, Session};

// 列出所有会话
let sessions = session_manager.list_sessions().await?;

// 加载会话到内存
let session = session_manager.load(session_id).await?;

// 获取会话中的线程
let thread = session.get_thread(&thread_id)?;
```

## 依赖关系

### 上游依赖
- `argus-protocol`：SessionId、ThreadId 等核心类型
- `argus-thread`：Thread 实现
- `argus-template`：TemplateManager
- `argus-tool`：ToolManager

### 下游消费者
- `argus-wing`：应用入口
- `cli`：命令行界面

## 设计原则

### 1. 惰性加载
- 会话数据按需从数据库加载
- 避免启动时加载所有会话到内存

### 2. 多层存储
- 内存：DashMap 缓存活跃会话
- 持久化：SQLite 存储会话元数据

### 3. Session 聚合
- Session 是 Thread 的聚合容器
- 提供会话级别的操作（列出线程、统计等）
