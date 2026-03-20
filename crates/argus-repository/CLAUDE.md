# Argus-Repository 持久化层

> 特性：持久化层，提供 LLM Provider 和其他实体数据的 SQLite 存储实现。

## 模块结构

```
src/
├── lib.rs           # 公共 API 导出
├── error.rs        # DbError 错误类型
├── traits.rs        # Repository trait 定义
├── types.rs        # 领域类型
└── sqlite/
    ├── mod.rs      # SQLite 实现
    ├── db.rs       # 数据库连接
    ├── init.rs     # 初始化
    ├── resolver.rs # Provider 解析
    └── cleaner.rs  # 清理任务
```

## 核心概念

### 1. Repository Traits

**Repository traits** 定义数据访问接口：

```rust
// 仓储接口定义
pub trait LlmProviderRepository {
    async fn get_provider(&self, id: &ProviderId) -> Result<ProviderRecord, DbError>;
    async fn list_providers(&self) -> Result<Vec<ProviderRecord>, DbError>;
}
```

### 2. SQLite 实现

**ArgusSqlite** 提供 SQLite 存储：

```rust
pub struct ArgusSqlite {
    pool: SqlitePool,
}

// 连接数据库
let db = ArgusSqlite::connect_path("argus.db").await?;

// 执行迁移
ArgusSqlite::migrate(&db.pool).await?;
```

### 3. Domain Types

- `ProviderRecord`：Provider 记录
- `AgentRecord`：Agent 配置记录

## 公共 API

```rust
use argus_repository::{ArgusSqlite, connect_path, migrate};

// 连接数据库
let pool = connect_path("argus.db").await?;

// 执行迁移
migrate(&pool).await?;

// 获取 Provider
let provider = repository.get_provider(&provider_id).await?;
```

## 依赖关系

### 上游依赖
- `argus-protocol`：核心类型
- `argus-llm`：Provider 实现

### 下游消费者
- `argus-wing`：应用入口
- `cli`：命令行界面

## 设计原则

### 1. Trait 抽象
- Repository 通过 trait 抽象数据访问
- 支持多种存储实现（SQLite 等）

### 2. 迁移管理
- 使用 sqlx migrate 管理数据库迁移
