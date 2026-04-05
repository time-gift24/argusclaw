# Argus-Wing

> 特性：面向 Tauri 桌面的入口模块，封装 AppContext 和 GraphQL API。

## 模块结构

```
src/
├── lib.rs           # 公共 API 导出（ArgusWing facade）
├── db.rs           # 数据库路径解析
├── init.rs         # 追踪初始化（init_tracing）
└── resolver.rs     # ProviderManagerResolver
```

## 核心概念

### 1. ArgusWing Facade

**ArgusWing** 是整个应用的核心门面：

```rust
pub struct ArgusWing {
    // 内部组合各个 argus-* 模块
}
```

**唯一入口点**：
- `ArgusWing::init()`：初始化应用
- `ArgusWing::with_pool()`：带数据库连接池初始化

### 2. ArgusWing 结构

**ArgusWing** 组合了以下管理器：

```rust
pub struct ArgusWing {
    pool: SqlitePool,
    provider_manager: Arc<ProviderManager>,
    template_manager: Arc<TemplateManager>,
    session_manager: Arc<SessionManager>,
    tool_manager: Arc<ToolManager>,
    job_manager: Arc<JobManager>,
    account_manager: Arc<AccountManager>,
}
```

### 3. 公共 API

**argus-wing 是 facade**，组合所有 argus-* 模块，通过单一 `ArgusWing` 结构暴露统一 API：

```rust
use argus_wing::ArgusWing;

// 初始化（自动完成数据库迁移、模板初始化等）
let wing = ArgusWing::init(None).await?;

// Provider CRUD
let providers = wing.list_providers().await?;
wing.upsert_provider(record).await?;

// Template CRUD
let templates = wing.list_templates().await?;

// Session/Thread 管理
let session_id = wing.create_session("My Session").await?;
let thread_id = wing.create_thread(session_id, template_id, None).await?;
wing.send_message(session_id, thread_id, "Hello".to_string()).await?;

// 订阅事件
let mut rx = wing.subscribe(session_id, thread_id).await?;
```

## 设计原则

### 1. Facade 模式
- 是 cli 和 desktop 的唯一入口
- 内部组合各个模块，不包含核心逻辑

### 2. 单一职责
- 仅负责组合，不负责实现
- 各模块保持独立
