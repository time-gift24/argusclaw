# Argus-Log 日志

> 特性：日志模块，记录会话和线程的生命周期事件。

## 模块结构

```
src/
├── lib.rs           # 公共 API 导出
├── repository.rs    # 日志仓储
└── models.rs        # 日志模型
```

## 核心概念

### 1. 日志模型

**LogRecord** 记录事件：

```rust
pub struct LogRecord {
    pub session_id: SessionId,
    pub thread_id: Option<ThreadId>,
    pub event: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}
```

### 2. LogRepository

**LogRepository** 管理日志持久化：

```rust
pub struct LogRepository {
    pool: SqlitePool,
}
```

## 公共 API

```rust
use argus_log::{LogRepository, LogRecord};

// 创建日志记录
let log = LogRecord {
    session_id,
    thread_id: Some(thread_id),
    event: "TurnCompleted".to_string(),
    metadata: serde_json::json!({}),
    created_at: Utc::now(),
};

repository.insert(log).await?;
```

## 依赖关系

### 上游依赖
- `argus-protocol`：SessionId、ThreadId

### 下游消费者
- `argus-session`：记录会话事件

## 设计原则

### 1. 事件记录
- 记录会话和线程的生命周期事件
- 支持查询历史事件
