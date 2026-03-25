# Argus-Template 模板

> 特性：模板模块，提供可配置的 Agent 和 Tool 模板。

## 模块结构

```
src/
├── lib.rs           # 公共 API 导出
├── config.rs        # 模板配置
├── manager.rs       # TemplateManager：模板管理
└── generated_agents.rs  # 自动生成的 Agent 模板
```

## 核心概念

### 1. TemplateManager

**TemplateManager** 管理 Agent 和 Tool 模板：

```rust
pub struct TemplateManager {
    // 模板存储
}
```

### 2. AgentRecord

**AgentRecord** 是从模板生成的 Agent 配置：

```rust
pub use argus_protocol::AgentRecord;
```

## 公共 API

```rust
use argus_template::{TemplateManager, AgentRecord};

let manager = TemplateManager::new();

// 获取 Agent 模板
let agent = manager.get_agent("default").await?;
```

## 依赖关系

### 上游依赖
- `argus-protocol`：`AgentRecord` 类型定义

### 下游消费者
- `argus-session`：使用模板创建会话

## 设计原则

### 1. 模板驱动
- Agent 配置通过模板定义
- 支持运行时模板参数化
