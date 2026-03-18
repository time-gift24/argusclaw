# Claw 迁移到 ArgusWing 设计文档

**日期:** 2026-03-18
**状态:** 设计阶段
**作者:** AI Assistant

## 1. 背景

ArgusWing 项目正在进行从单 crate (claw) 到多 crate 架构的迁移。目前已有 11 个功能模块被提取为独立 crates（argus-protocol、argus-session、argus-thread、argus-turn 等），并创建了统一入口点 `argus-wing`。

**当前状态：**
- ✅ Desktop 已迁移到 argus-wing
- ❌ CLI 仍依赖 claw
- ⚠️  claw 包含生产 API 和 dev/testing API 混合

**迁移目标：**
- 完全删除 claw crate
- CLI 生产命令使用 argus-wing
- CLI dev 命令使用新的 argus-dev crate

## 2. 架构设计

### 2.1 最终 Crate 结构

```
crates/
├── argus-wing/          # 生产 API（已存在，增强）
│   └── src/lib.rs       # ArgusWing 统一入口
│
├── argus-dev/           # 开发测试工具（新建）
│   ├── src/lib.rs       # DevTools 入口
│   ├── src/turn.rs      # Turn 执行测试
│   ├── src/workflow.rs  # Workflow/Job 管理
│   └── src/scheduler.rs # Cron 调度器
│
├── argus-protocol/      # 共享类型（已存在）
├── argus-session/       # Session 管理（已存在）
├── argus-thread/        # Thread 执行（已存在）
├── argus-turn/          # Turn 逻辑（已存在）
├── argus-llm/           # LLM 提供商（已存在）
├── argus-tool/          # 工具管理（已存在）
├── argus-approval/      # 审批系统（已存在）
├── argus-repository/    # 数据库层（已存在）
├── argus-template/      # 模板管理（已存在）
│
├── cli/                 # CLI 前端（迁移中）
│   ├── src/main.rs      # 生产命令 → 使用 ArgusWing
│   └── src/dev/         # Dev 命令 → 使用 argus-dev
│
└── desktop/             # 桌面前端（已使用 ArgusWing）
```

### 2.2 职责分离

**argus-wing (生产 API):**
- Provider CRUD
- Template CRUD
- Session/Thread 管理
- Messaging 和订阅
- Approval 管理
- **不包含** dev/testing 工具

**argus-dev (开发工具):**
- Turn 执行测试
- Workflow/Job 管理
- Cron 调度器
- 依赖 argus-wing 提供核心功能

## 3. 实施方案

### 3.1 第一阶段：清理未使用代码

**目标:** 删除 claw 中未使用的模块

**操作:**
1. 删除 `crates/claw/src/api/` - GraphQL API（无引用）
2. 检查并删除其他死代码

**影响:** 无，这些代码未被使用

### 3.2 第二阶段：创建 argus-dev crate

**目标:** 提供统一的开发测试工具入口

**Crate 配置:**
```toml
[package]
name = "argus-dev"
version = "0.1.0"
edition = "2021"

[dependencies]
argus-wing = { path = "../argus-wing" }
argus-turn = { path = "../argus-turn" }
argus-repository = { path = "../argus-repository" }
argus-protocol = { path = "../argus-protocol" }
anyhow = "1"
tokio = { version = "1", features = ["sync"] }
sqlx = { version = "0.8", features = ["sqlite", "runtime-tokio"] }
```

**核心结构:**
```rust
// crates/argus-dev/src/lib.rs
use std::sync::Arc;
use argus_wing::ArgusWing;
use argus_turn::{TurnConfig, TurnInput, TurnOutput};
use argus_protocol::{Result, WorkflowId, WorkflowRecord, JobRecord};

/// 开发测试工具统一入口
pub struct DevTools {
    pool: SqlitePool,
    wing: Arc<ArgusWing>,
}

impl DevTools {
    /// 初始化 DevTools
    pub async fn init(database_path: Option<&str>) -> Result<Arc<Self>> {
        let wing = ArgusWing::init(database_path).await?;
        Ok(Arc::new(Self {
            pool: wing.pool.clone(),
            wing,
        }))
    }

    // === Turn Execution API ===
    pub async fn execute_turn(&self, input: TurnInput, config: TurnConfig) -> Result<TurnOutput>;

    // === Workflow API ===
    pub async fn create_workflow(&self, name: &str) -> Result<WorkflowId>;
    pub async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>>;
    pub async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>>;
    pub async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool>;

    // === Job API ===
    pub async fn create_job(&self, job: JobRecord) -> Result<()>;
    pub async fn list_jobs(&self, workflow_id: &str) -> Result<Vec<JobRecord>>;

    // === Scheduler API ===
    pub fn scheduler(&self) -> &Arc<Scheduler>;
}
```

**依赖关系:**
```
argus-dev → argus-wing (复用初始化和管理器)
          → argus-turn (Turn execution)
          → argus-repository (Workflow/Job repositories)
```

### 3.3 第三阶段：迁移 CLI 生产命令到 ArgusWing

**目标:** 更新 `main.rs` 和 `agent.rs` 使用 ArgusWing API

**主要变更:**

#### 3.3.1 初始化 (main.rs)
```rust
// 旧代码
use claw::AppContext;

let ctx = AppContext::init(Some(db_url)).await?;

// 新代码
use argus_wing::ArgusWing;

let wing = ArgusWing::init(Some(db_url)).await?;
```

#### 3.3.2 Agent 创建 (agent.rs)
```rust
// 旧代码
let agent_id = ctx.create_default_agent_with_approval(tools, auto_approve).await?;
let thread_id = ctx.create_thread(&agent_id, ThreadConfig::default())?;

// 新代码
let session_id = wing.create_session("default-session").await?;
let template_id = wing.get_default_template().await?.unwrap();
let thread_id = wing.create_thread(session_id, template_id, None).await?;
```

#### 3.3.3 消息发送 (agent.rs)
```rust
// 旧代码
ctx.send_message(&agent_id, &thread_id, message).await?;

// 新代码
wing.send_message(session_id, thread_id, message).await?;
```

#### 3.3.4 订阅事件 (agent.rs)
```rust
// 旧代码
let mut rx = ctx.subscribe(&agent_id, &thread_id).await?;

// 新代码
let mut rx = wing.subscribe(session_id, thread_id).await?;
```

#### 3.3.5 审批 (agent.rs)
```rust
// 旧代码
ctx.resolve_approval(&agent_id, request_id, decision, resolved_by)?;

// 新代码
wing.resolve_approval(request_id, decision, resolved_by)?;
```

**影响的文件:**
- `crates/cli/src/main.rs`
- `crates/cli/src/agent.rs`
- `crates/cli/src/provider.rs`
- `crates/cli/src/lib.rs`
- `crates/cli/Cargo.toml` (更新依赖)

### 3.4 第四阶段：迁移 CLI dev 命令到 argus-dev

**目标:** 更新 `src/dev/` 所有模块使用 argus-dev

**主要变更:**

#### 3.4.1 Turn 命令 (dev/turn.rs)
```rust
// 旧代码
use claw::turn::{TurnConfig, TurnInputBuilder, execute_turn};
use claw::AppContext;

pub async fn run_turn_command(ctx: AppContext, command: TurnCommand) -> Result<()> {
    let input = TurnInputBuilder::default()
        .provider(provider)
        .messages(vec![ChatMessage::user(message)])
        .build()?;
    let output = execute_turn(input, TurnConfig::default()).await?;
}

// 新代码
use argus_dev::DevTools;
use argus_turn::{TurnConfig, TurnBuilder};

pub async fn run_turn_command(dev_tools: Arc<DevTools>, command: TurnCommand) -> Result<()> {
    let turn = TurnBuilder::default()
        .provider_id(provider_id)
        .messages(vec![ChatMessage::user(message)])
        .build()?;
    let output = dev_tools.execute_turn(turn, TurnConfig::default()).await?;
}
```

#### 3.4.2 Workflow 命令 (dev/workflow.rs)
```rust
// 旧代码
use claw::{SqliteWorkflowRepository, JobRepository};

let repo = SqliteWorkflowRepository::new(pool);
let workflow = WorkflowRecord { /* ... */ };
repo.create_workflow(&workflow).await?;

// 新代码
use argus_dev::DevTools;

let workflow_id = dev_tools.create_workflow("my-workflow").await?;
// dev_tools 内部管理 repository
```

#### 3.4.3 初始化 (dev/mod.rs)
```rust
// 旧代码
use claw::AppContext;

pub async fn run(ctx: AppContext, command: DevCommand) -> Result<()> {
    match command {
        DevCommand::Turn(cmd) => run_turn_command(ctx, cmd).await,
        // ...
    }
}

// 新代码
use argus_dev::DevTools;
use std::sync::Arc;

pub async fn run(dev_tools: Arc<DevTools>, command: DevCommand) -> Result<()> {
    match command {
        DevCommand::Turn(cmd) => run_turn_command(dev_tools.clone(), cmd).await,
        // ...
    }
}
```

#### 3.4.4 Main-dev 入口 (main-dev.rs)
```rust
// 旧代码
let ctx = AppContext::init(Some(db_url)).await?;
run(ctx, cli.command).await?;

// 新代码
let dev_tools = DevTools::init(Some(db_url)).await?;
run(dev_tools, cli.command).await?;
```

**影响的文件:**
- `crates/cli/src/dev/mod.rs`
- `crates/cli/src/dev/turn.rs`
- `crates/cli/src/dev/workflow.rs`
- `crates/cli/src/dev/llm.rs`
- `crates/cli/src/dev/approval.rs`
- `crates/cli/src/dev/config.rs`
- `crates/cli/src/main-dev.rs`
- `crates/cli/Cargo.toml` (添加 argus-dev 依赖)

### 3.5 第五阶段：删除 claw crate

**前提条件:**
- ✅ CLI 生产命令已迁移到 ArgusWing
- ✅ CLI dev 命令已迁移到 argus-dev
- ✅ Desktop 已使用 ArgusWing（已完成）
- ✅ 所有测试通过
- ✅ 无剩余代码引用 claw

**操作:**
1. 从 `Cargo.toml` workspace 移除 `members = ["claw"]`
2. 删除 `crates/claw/` 目录
3. 更新 `CLAUDE.md` 和相关文档
4. 运行完整测试套件验证

## 4. API 映射表

### 4.1 生产 API 映射 (AppContext → ArgusWing)

| 功能 | AppContext | ArgusWing |
|------|-----------|-----------|
| 初始化 | `AppContext::init(db_url)` | `ArgusWing::init(db_url)` |
| 创建 Agent | `create_default_agent_with_approval()` | `create_session() + create_thread()` |
| 发送消息 | `send_message(&agent_id, &thread_id, msg)` | `send_message(session_id, thread_id, msg)` |
| 订阅事件 | `subscribe(&agent_id, &thread_id)` | `subscribe(session_id, thread_id)` |
| 审批 | `resolve_approval(&agent_id, id, dec, by)` | `resolve_approval(id, dec, by)` |
| Provider | `list_providers()`, `upsert_provider()` | 相同方法 |
| Template | `create_agent_template()` | `upsert_template()` |

### 4.2 Dev API 映射 (claw → argus-dev)

| 功能 | claw | argus-dev |
|------|------|-----------|
| Turn 执行 | `claw::turn::execute_turn()` | `DevTools::execute_turn()` |
| Workflow | `claw::SqliteWorkflowRepository` | `DevTools::create_workflow()` 等 |
| Job | `claw::JobRepository` | `DevTools::create_job()` 等 |
| Scheduler | `claw::Scheduler::new()` | `DevTools::scheduler()` |

## 5. 测试策略

### 5.1 单元测试
- 每个阶段完成后运行 `cargo test --p <crate>`
- 确保新 crate 的测试覆盖率

### 5.2 集成测试
- 测试 CLI 生产命令：`cargo run --bin arguswing -- agent chat`
- 测试 CLI dev 命令：`cargo run --bin arguswing-dev -- turn test "hello"`
- 测试 Desktop 应用（已使用 ArgusWing）

### 5.3 回归测试
- 完整测试套件：`cargo test --workspace`
- Clippy 检查：`cargo clippy --workspace`
- Fmt 检查：`cargo fmt --all --check`

## 6. 风险和缓解

### 6.1 API 不兼容性
**风险:** ArgusWing API 与 AppContext 差异较大，迁移可能引入 bug

**缓解:**
- 逐步迁移，每步都测试
- 保持详细日志便于调试
- 保留 claw 直到完全验证迁移成功

### 6.2 功能缺失
**风险:** ArgusWing 可能缺少某些 AppContext 的功能

**缓解:**
- 在迁移前对比 API 完整性
- 必要时扩展 ArgusWing API
- 使用 feature flag 控制新功能

### 6.3 Dev 工具复杂性
**风险:** argus-dev 可能过度设计，引入不必要的复杂性

**缓解:**
- 保持 argus-dev 简单，主要是 re-export 和薄包装
- 依赖 argus-wing 而非重新实现
- 只迁移实际使用的 dev 功能

## 7. 实施顺序

**并行推进策略:**

1. **第一步（并行）:**
   - 清理 claw 未使用代码
   - 创建 argus-dev crate 基础结构

2. **第二步（并行）:**
   - 迁移 CLI 生产命令到 ArgusWing
   - 迁移 CLI dev 命令到 argus-dev

3. **第三步（验证）:**
   - 运行完整测试套件
   - 手动测试 CLI 和 Desktop

4. **第四步（清理）:**
   - 删除 claw crate
   - 更新文档
   - 最终验证

## 8. 成功标准

迁移成功的标志：
- [ ] CLI 生产命令功能正常（agent chat, provider 管理）
- [ ] CLI dev 命令功能正常（turn test, workflow）
- [ ] Desktop 应用功能正常
- [ ] 所有测试通过（`cargo test --workspace`）
- [ ] 无 clippy 警告
- [ ] claw crate 完全删除
- [ ] 文档更新完整

## 9. 后续优化

迁移完成后的可选优化：
- 重构 ArgusWing API 以消除历史遗留
- 扩展 argus-dev 功能（如性能测试工具）
- 统一错误处理模式
- 添加更多集成测试
