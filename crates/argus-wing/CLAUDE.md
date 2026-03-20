# Argus-Wing

> 特性：面向 Tauri 桌面的入口模块，封装 AppContext 和 GraphQL API。

## 模块结构

```
src/
├── lib.rs           # 公共 API 导出（ArgusWing）
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

### 2. AppContext

**AppContext** 提供应用级上下文：

```rust
pub struct AppContext {
    pub session_manager: Arc<SessionManager>,
    pub provider_manager: Arc<ProviderManager>,
    pub tool_manager: Arc<ToolManager>,
    // ...
}
```

## 公共 API

```rust
use argus_wing::ArgusWing;

// 初始化
let app = ArgusWing::init().await?;

// 获取上下文
let ctx = app.context();
let session = ctx.session_manager.load(session_id).await?;
```

## 依赖关系

**argus-wing 是 facade**，组合所有 argus-* 模块：
- `argus-protocol`：核心类型
- `argus-session`：会话管理
- `argus-thread`：线程管理
- `argus-turn`：轮次执行
- `argus-llm`：LLM 抽象
- `argus-approval`：审批系统
- `argus-tool`：工具注册表
- `argus-repository`：持久化
- `argus-auth`：认证
- `argus-crypto`：加密

## 设计原则

### 1. Facade 模式
- 是 cli 和 desktop 的唯一入口
- 内部组合各个模块，不包含核心逻辑

### 2. 单一职责
- 仅负责组合，不负责实现
- 各模块保持独立
