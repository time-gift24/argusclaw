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
- **需要扩展:** 添加默认模板查找和审批配置支持

**argus-dev (开发工具):**
- Turn 执行测试
- Workflow/Job 管理
- Cron 调度器
- 依赖 argus-wing 提供核心功能

**关键架构变化:**
- **Agent 模型 → Session 模型**: claw 使用 Agent (包含多个 Thread)，argus-wing 使用 Session (包含多个 Thread)
- **默认模板**: claw 通过 `find_template_by_display_name("ArgusWing")` 查找，ArgusWing 需要添加类似方法
- **审批配置**: claw 在创建 Agent 时配置审批工具，argus-wing 需要添加此功能

## 3. 实施方案

### 3.1 第一阶段：清理未使用代码

**目标:** 删除 claw 中未使用的模块

**操作:**
1. 删除 `crates/claw/src/api/` - GraphQL API（无引用）
2. 检查并删除其他死代码

**影响:** 无，这些代码未被使用

### 3.2 第二阶段：扩展 ArgusWing 和创建 argus-dev crate

#### 3.2.1 扩展 ArgusWing API

**目标:** 添加缺失的功能以支持 CLI 迁移

**需要添加的方法:**

```rust
// crates/argus-wing/src/lib.rs

impl ArgusWing {
    /// 查找默认模板（"ArgusWing"）
    /// claw 使用 `find_template_by_display_name("ArgusWing")`
    pub async fn get_default_template(&self) -> Result<Option<AgentRecord>> {
        let templates = self.list_templates().await?;
        Ok(templates.into_iter()
            .find(|t| t.display_name == "ArgusWing"))
    }

    /// 创建会话并配置审批（类似 claw 的 create_default_agent_with_approval）
    pub async fn create_session_with_approval(
        &self,
        name: &str,
        approval_tools: Vec<String>,
        auto_approve: bool,
    ) -> Result<(SessionId, ThreadId)> {
        let session_id = self.create_session(name).await?;

        // 获取默认模板
        let template = self.get_default_template().await?
            .ok_or_else(|| ArgusError::TemplateError {
                reason: "Default template 'ArgusWing' not found".to_string(),
            })?;

        // 配置审批策略
        if !approval_tools.is_empty() {
            let policy = ApprovalPolicy::new()
                .require_approval_for(approval_tools)
                .auto_approve(auto_approve);
            self.approval_manager.update_policy(policy);
        }

        // 创建线程
        let thread_id = self.create_thread(session_id, template.id, None).await?;

        Ok((session_id, thread_id))
    }
}
```

#### 3.2.2 创建 argus-dev crate

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
use argus_repository::{WorkflowRepository, JobRepository, SqliteWorkflowRepository, SqliteJobRepository};

/// 开发测试工具统一入口
pub struct DevTools {
    pool: SqlitePool,
    wing: Arc<ArgusWing>,
    workflow_repo: Arc<SqliteWorkflowRepository>,
    job_repo: Arc<dyn JobRepository>,
}

impl DevTools {
    /// 初始化 DevTools
    ///
    /// 使用独立数据库进行开发测试，默认使用 `./tmp/dev-workflow.sqlite`
    pub async fn init(database_path: Option<&str>) -> Result<Arc<Self>> {
        // 使用 ArgusWing 的初始化逻辑
        let wing = ArgusWing::init(database_path).await?;
        let pool = wing.pool.clone();

        // 创建 workflow 和 job repositories
        let workflow_repo = Arc::new(SqliteWorkflowRepository::new(pool.clone()));
        let job_repo: Arc<dyn JobRepository> = Arc::new(SqliteJobRepository::new(pool));

        Ok(Arc::new(Self {
            pool,
            wing,
            workflow_repo,
            job_repo,
        }))
    }

    /// 使用自定义数据库初始化（用于 workflow 测试）
    pub async fn init_with_db(database_url: &str) -> Result<Arc<Self>> {
        let wing = ArgusWing::init(Some(database_url)).await?;
        let pool = wing.pool.clone();

        let workflow_repo = Arc::new(SqliteWorkflowRepository::new(pool.clone()));
        let job_repo: Arc<dyn JobRepository> = Arc::new(SqliteJobRepository::new(pool));

        Ok(Arc::new(Self {
            pool,
            wing,
            workflow_repo,
            job_repo,
        }))
    }

    // === 访问 ArgusWing ===
    pub fn wing(&self) -> &Arc<ArgusWing> {
        &self.wing
    }

    // === Turn Execution API ===
    pub async fn execute_turn(&self, input: TurnInput, config: TurnConfig) -> Result<TurnOutput> {
        // 使用 argus-turn 的 execute_turn 函数
        argus_turn::execute_turn(input, config).await
    }

    // === Workflow API ===
    pub async fn create_workflow(&self, name: &str) -> Result<WorkflowId> {
        let id = WorkflowId::new(uuid::Uuid::new_v4().to_string());
        let workflow = WorkflowRecord {
            id: id.clone(),
            name: name.to_string(),
            status: argus_protocol::WorkflowStatus::Pending,
        };
        self.workflow_repo.create_workflow(&workflow).await?;
        Ok(id)
    }

    pub async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>> {
        self.workflow_repo.list_workflows().await
    }

    pub async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>> {
        self.workflow_repo.get_workflow(id).await
    }

    pub async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool> {
        self.workflow_repo.delete_workflow(id).await
    }

    // === Job API ===
    pub async fn create_job(&self, job: JobRecord) -> Result<()> {
        self.job_repo.create(&job).await
    }

    pub async fn list_jobs(&self, workflow_id: &str) -> Result<Vec<JobRecord>> {
        self.job_repo.list_by_group(workflow_id).await
    }

    pub async fn update_job_status(
        &self,
        id: &JobId,
        status: argus_protocol::WorkflowStatus,
        started_at: Option<DateTime<Utc>>,
        finished_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.job_repo.update_status(id, status, started_at, finished_at).await
    }
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

**关键变化:** Agent 模型变为 Session 模型

```rust
// 旧代码 (claw)
let agent_id = ctx.create_default_agent_with_approval(
    approval_tools.clone(),
    auto_approve,
).await?;
let thread_id = ctx.create_thread(&agent_id, ThreadConfig::default())?;

// 发送消息
ctx.send_message(&agent_id, &thread_id, message).await?;
ctx.subscribe(&agent_id, &thread_id).await?;
ctx.resolve_approval(&agent_id, request_id, decision, resolved_by)?;

// 新代码 (argus-wing)
let (session_id, thread_id) = wing.create_session_with_approval(
    "default-session",
    approval_tools.clone(),
    auto_approve,
).await?;

// 发送消息
wing.send_message(session_id, thread_id, message).await?;
wing.subscribe(session_id, thread_id).await?;
wing.resolve_approval(request_id, decision, resolved_by)?;
```

**说明:**
- `agent_id` 被 `session_id` 替代
- `create_session_with_approval()` 一次性创建 session 和 thread，并配置审批
- 后续 API 调用不再需要 `agent_id`

#### 3.3.3-3.3.5 消息、订阅和审批

这些 API 的变化较小，主要是参数调整：

```rust
// 消息发送
// 旧: ctx.send_message(&agent_id, &thread_id, message).await?;
// 新: wing.send_message(session_id, thread_id, message).await?;

// 订阅事件
// 旧: let mut rx = ctx.subscribe(&agent_id, &thread_id).await?;
// 新: let mut rx = wing.subscribe(session_id, thread_id).await?;

// 审批
// 旧: ctx.resolve_approval(&agent_id, request_id, decision, resolved_by)?;
// 新: wing.resolve_approval(request_id, decision, resolved_by)?;
```

**关键点:**
- `agent_id` 参数被移除（session 隐含在 thread 上下文中）
-审批只需要 `request_id`，不再需要 `agent_id`

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

**关键变化:** 使用正确的 API

```rust
// 旧代码 (claw)
use claw::turn::{TurnConfig, TurnInputBuilder, execute_turn};
use claw::{AppContext, ChatMessage, LlmProvider};

pub async fn run_turn_command(ctx: AppContext, command: TurnCommand) -> Result<()> {
    let provider = ctx.get_default_provider().await?;

    let input = TurnInputBuilder::default()
        .provider(provider)
        .messages(vec![ChatMessage::user(message)])
        .system_prompt(system_prompt)
        .tool_manager(tool_manager)
        .tool_ids(tools)
        .build()?;

    let output = execute_turn(input, TurnConfig::default()).await?;
}

// 新代码 (argus-dev + argus-turn)
use argus_dev::DevTools;
use argus_turn::{TurnConfig, TurnInputBuilder, execute_turn};
use argus_protocol::{ChatMessage, ProviderId};

pub async fn run_turn_command(dev_tools: Arc<DevTools>, command: TurnCommand) -> Result<()> {
    // 通过 DevTools 访问 ArgusWing 获取 provider
    let wing = dev_tools.wing();
    let provider_record = wing.get_default_provider_record().await?;

    // 构建 TurnInput (注意: 使用 TurnInputBuilder)
    let input = TurnInputBuilder::default()
        .provider_id(ProviderId::new(provider_record.id.into_inner()))
        .messages(vec![ChatMessage::user(message)])
        .system_prompt(system_prompt)
        .tool_manager(tool_manager)
        .tool_ids(tools)
        .build()?;

    // 执行 Turn (使用 argus-turn 的函数)
    let output = dev_tools.execute_turn(input, TurnConfig::default()).await?;
}
```

**说明:**
- `TurnInputBuilder` 来自 `argus-turn` crate
- `execute_turn` 是 `argus_turn::execute_turn` 函数
- Provider 通过 `DevTools::wing()` 访问 ArgusWing 获取

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

## 4. 数据库初始化策略

### 4.1 生产命令 (main.rs)

**目标:** 使用用户指定的数据库或默认路径

```rust
// crates/cli/src/main.rs
let db_path = resolve_db_path(false);  // 生产环境: ~/.arguswing/sqlite.db
let db_url = db_path_to_url(&db_path);
let wing = ArgusWing::init(Some(&db_url)).await?;
```

### 4.2 Dev 命令 (main-dev.rs)

**目标:** 使用独立开发数据库，避免污染生产数据

```rust
// crates/cli/src/main-dev.rs
// 选项 1: 使用环境变量
let db_url = std::env::var("DEV_DATABASE_URL")
    .unwrap_or_else(|_| "sqlite:./tmp/dev.sqlite".to_string());
let dev_tools = DevTools::init(Some(&db_url)).await?;

// 选项 2: workflow 命令使用独立数据库
// (workflow 命令当前使用 `./tmp/workflow-dev.sqlite`)
let dev_tools = DevTools::init_with_db(&workflow_db_url).await?;
```

**策略:**
- **生产命令:** 使用 `~/.arguswing/sqlite.db`
- **Dev 命令:** 使用 `./tmp/dev.sqlite` 或环境变量指定
- **Workflow 测试:** 使用独立的 `./tmp/workflow-dev.sqlite`

## 5. API 映射表

### 5.1 生产 API 映射 (AppContext → ArgusWing)

| 功能 | AppContext (claw) | ArgusWing | 说明 |
|------|-----------|-----------|------|
| 初始化 | `AppContext::init(db_url)` | `ArgusWing::init(db_url)` | ✅ 相同 |
| 获取默认模板 | `get_default_agent_template()` | `get_default_template()` | ⚠️ 需要添加到 ArgusWing |
| 创建 Agent | `create_default_agent_with_approval(tools, auto)` | `create_session_with_approval(name, tools, auto)` | ⚠️ 需要添加，返回 (SessionId, ThreadId) |
| 发送消息 | `send_message(&agent_id, &thread_id, msg)` | `send_message(session_id, thread_id, msg)` | ⚠️ 移除 agent_id 参数 |
| 订阅事件 | `subscribe(&agent_id, &thread_id)` | `subscribe(session_id, thread_id)` | ⚠️ 移除 agent_id 参数 |
| 审批 | `resolve_approval(&agent_id, id, dec, by)` | `resolve_approval(id, dec, by)` | ⚠️ 移除 agent_id 参数 |
| Provider CRUD | `list_providers()`, `upsert_provider()` | 相同方法 | ✅ 相同 |
| Template CRUD | `create_agent_template()`, `get_default_agent_template()` | `upsert_template()`, `get_default_template()` | ⚠️ 需要添加 get_default_template |
| 获取 Provider | `get_provider(id)`, `get_default_provider()` | `get_provider_record(id)`, `get_default_provider_record()` | ⚠️ 方法名不同 |

**关键变化:**
- `agent_id` 参数被移除，由 `session_id` 替代
- 需要在 ArgusWing 中添加 `get_default_template()` 和 `create_session_with_approval()` 方法

### 5.2 Dev API 映射 (claw → argus-dev)

| 功能 | claw | argus-dev | 说明 |
|------|------|-----------|------|
| Turn 执行 | `claw::turn::execute_turn(input, config)` | `DevTools::execute_turn(input, config)` | ✅ 包装 argus-turn::execute_turn |
| Turn Input 构建 | `claw::turn::TurnInputBuilder` | `argus_turn::TurnInputBuilder` | ✅ 直接使用 argus-turn |
| Workflow 创建 | `SqliteWorkflowRepository::new(pool)` + `create_workflow()` | `DevTools::create_workflow(name)` | ✅ 内部管理 repository |
| Workflow 列表 | `repo.list_workflows()` | `DevTools::list_workflows()` | ✅ 包装方法 |
| Job 创建 | `JobRepository::create(&job)` | `DevTools::create_job(job)` | ✅ 包装方法 |
| Job 状态更新 | `repo.update_status(id, status, ...)` | `DevTools::update_job_status(id, status, ...)` | ✅ 包装方法 |
| 访问 ArgusWing | N/A | `DevTools::wing()` | ✅ 访问生产 API |
| 数据库初始化 | `claw::sqlite::connect()` | `DevTools::init_with_db(url)` | ✅ 独立数据库支持 |

**关键优势:**
- DevTools 提供统一入口，不需要手动管理 repositories
- 可以访问 ArgusWing 的所有功能（provider, template 等）
- 支持独立数据库，避免污染生产数据

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

**缓解措施:**
1. **逐步迁移:** 每个阶段完成后运行测试
2. **保持详细日志:** 使用 `RUST_LOG=debug` 跟踪 API 调用
3. **保留 claw:** 直到完全验证迁移成功
4. **编译时验证:** 利用 Rust 类型系统捕获错误

**验证步骤:**
- [ ] 迁移后代码能编译通过
- [ ] 所有单元测试通过
- [ ] 手动测试 CLI 命令
- [ ] 对比迁移前后的输出

### 6.2 功能缺失

**风险:** ArgusWing 可能缺少某些 AppContext 的功能

**缓解措施:**
1. **API 完整性检查:** 迁移前对比所有 API
2. **扩展 ArgusWing:** 添加缺失的方法（见 3.2.1）
3. **Feature gates:** 使用 feature flag 控制新功能
4. **回滚计划:** 保留 claw 直到完全验证

**需要添加的功能:**
- [ ] `ArgusWing::get_default_template()`
- [ ] `ArgusWing::create_session_with_approval()`
- [ ] `argus_template::TemplateManager::find_by_display_name()` (可选)

### 6.3 Dev 工具复杂性

**风险:** argus-dev 可能过度设计，引入不必要的复杂性

**缓解措施:**
1. **保持简单:** DevTools 主要是薄包装
2. **复用而非重写:** 依赖 argus-wing 和其他 crates
3. **只迁移使用的功能:** 不需要实现所有 claw dev 功能
4. **文档化:** 清晰的 API 文档和示例

**验证标准:**
- DevTools 代码行数 < 500 行
- 无复杂的状态管理
- 清晰的依赖关系

### 6.4 数据库迁移

**风险:** Dev 命令可能污染生产数据

**缓解措施:**
1. **独立数据库:** Dev 命令使用 `./tmp/dev.sqlite`
2. **环境变量:** 支持通过 `DEV_DATABASE_URL` 覆盖
3. **默认路径明确:** Dev 命令默认不使用生产路径
4. **文档说明:** 在 CLI help 中说明数据库位置

### 6.5 并行执行安全

**风险:** 同时修改多个模块可能导致冲突

**缓解措施:**
1. **清晰边界:** 生产命令和 dev 命令使用不同 crates
2. **独立测试:** 每个部分独立测试
3. **代码审查:** 每个阶段提交前审查
4. **频繁集成:** 定期合并主分支冲突

## 7. 实施顺序

**并行推进策略:**

### 第一步（并行）：准备阶段
- [ ] **清理 claw 未使用代码**
  - 删除 `crates/claw/src/api/`
  - 运行 `cargo test --p claw` 验证
- [ ] **创建 argus-dev crate 基础结构**
  - 创建 `crates/argus-dev/` 目录
  - 添加 `Cargo.toml`
  - 实现 `DevTools::init()` 基础结构
  - 运行 `cargo test --p argus-dev` 验证

### 第二步（并行）：核心迁移
- [ ] **扩展 ArgusWing API**
  - 添加 `get_default_template()` 方法
  - 添加 `create_session_with_approval()` 方法
  - 运行 `cargo test --p argus-wing` 验证
- [ ] **迁移 CLI 生产命令到 ArgusWing**
  - 更新 `main.rs` 初始化
  - 更新 `agent.rs` (agent chat 命令)
  - 更新 `provider.rs` (provider 命令)
  - 运行 `cargo run --bin arguswing -- agent chat` 手动测试
- [ ] **实现 argus-dev 功能**
  - 实现 `execute_turn()` 方法
  - 实现 workflow/job 相关方法
  - 添加 `init_with_db()` 方法
- [ ] **迁移 CLI dev 命令到 argus-dev**
  - 更新 `dev/turn.rs`
  - 更新 `dev/workflow.rs`
  - 更新 `dev/mod.rs` 和 `main-dev.rs`
  - 运行 `cargo run --bin arguswing-dev -- turn test "hello"` 手动测试

### 第三步（验证）：完整测试
- [ ] **运行完整测试套件**
  ```bash
  cargo test --workspace
  cargo clippy --workspace
  cargo fmt --all --check
  ```
- [ ] **手动测试 CLI**
  - 生产命令: `cargo run --bin arguswing -- agent chat`
  - Dev 命令: `cargo run --bin arguswing-dev -- turn test "hello"`
  - Provider: `cargo run --bin arguswing -- provider list`
- [ ] **手动测试 Desktop**
  - 启动 Desktop 应用
  - 测试 provider 管理
  - 测试 agent 对话
- [ ] **性能测试**
  - 对比迁移前后的启动时间
  - 对比内存占用

### 第四步（清理）：删除 claw
- [ ] **验证无剩余引用**
  ```bash
  grep -r "use claw" crates/
  grep -r "claw::" crates/
  ```
- [ ] **从 workspace 移除 claw**
  - 更新 `Cargo.toml` workspace members
  - 删除 `crates/claw/` 目录
- [ ] **更新文档**
  - 更新 `CLAUDE.md`
  - 更新 README
  - 更新架构图
- [ ] **最终验证**
  ```bash
  cargo build --workspace
  cargo test --workspace
  ```
- [ ] **提交并创建 PR**

### 每个阶段的退出标准

**第一阶段退出标准:**
- [ ] `crates/claw/src/api/` 已删除
- [ ] `crates/argus-dev/Cargo.toml` 已创建
- [ ] `cargo build --p argus-dev` 成功
- [ ] 无编译警告

**第二阶段退出标准:**
- [ ] ArgusWing 扩展方法已实现并测试
- [ ] CLI 生产命令功能正常（手动测试通过）
- [ ] CLI dev 命令功能正常（手动测试通过）
- [ ] 所有测试通过: `cargo test --workspace`

**第三阶段退出标准:**
- [ ] 完整测试套件通过
- [ ] 无 clippy 警告
- [ ] 手动测试清单全部完成
- [ ] 性能对比结果可接受

**第四阶段退出标准:**
- [ ] 无 claw 引用残留
- [ ] claw crate 已删除
- [ ] 文档已更新
- [ ] 最终构建和测试成功

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
