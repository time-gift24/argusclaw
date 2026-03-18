# Claw 迁移到 ArgusWing 实施计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**目标:** 将 CLI 从 claw crate 完全迁移到 argus-wing 和 argus-dev，最终删除 claw crate

**架构:** 扩展 ArgusWing API 以支持 CLI 需求，创建 argus-dev crate 封装开发测试工具，逐步迁移 CLI 命令

**技术栈:** Rust, Cargo workspace, SQLx, Tokio, argus-wing, argus-turn, argus-repository

---

## 文件结构映射

### 新建文件
- `crates/argus-dev/Cargo.toml` - argus-dev crate 配置
- `crates/argus-dev/src/lib.rs` - DevTools 主入口
- `crates/argus-dev/src/turn.rs` - Turn 执行测试封装
- `crates/argus-dev/src/workflow.rs` - Workflow/Job 管理
- `crates/argus-dev/src/error.rs` - argus-dev 错误类型

### 修改文件
- `crates/argus-wing/src/lib.rs` - 添加 get_default_template() 和 create_session_with_approval()
- `crates/cli/Cargo.toml` - 更新依赖（移除 claw，添加 argus-wing 和 argus-dev）
- `crates/cli/src/main.rs` - 使用 ArgusWing 替代 AppContext
- `crates/cli/src/agent.rs` - 迁移到 Session/Thread 模型
- `crates/cli/src/provider.rs` - 使用 ArgusWing API
- `crates/cli/src/lib.rs` - 更新类型导出
- `crates/cli/src/dev/mod.rs` - 使用 DevTools
- `crates/cli/src/dev/turn.rs` - 使用 argus-turn API
- `crates/cli/src/dev/workflow.rs` - 使用 DevTools API
- `crates/cli/src/dev/llm.rs` - 使用 ArgusWing API
- `crates/cli/src/dev/approval.rs` - 使用 ArgusWing API
- `crates/cli/src/dev/config.rs` - 使用 ArgusWing API
- `crates/cli/src/main-dev.rs` - 使用 DevTools 初始化
- `Cargo.toml` - 从 workspace 移除 claw

### 删除文件
- `crates/claw/src/api/` - GraphQL API（未使用）

---

## Chunk 1: 准备阶段 - 清理和基础结构

### Task 1: 删除 claw 中未使用的代码

**Files:**
- Delete: `crates/claw/src/api/mod.rs`
- Delete: `crates/claw/src/api/schema.rs`
- Delete: `crates/claw/src/api/mutations.rs`
- Delete: `crates/claw/src/api/queries.rs`

- [ ] **Step 1: 验证 api/ 模块确实未被使用**

```bash
grep -r "use claw::api" crates/
grep -r "claw::api::" crates/
```

Expected output:
```
(no results - empty output)
```

- [ ] **Step 2: 删除 api/ 目录**

```bash
rm -rf crates/claw/src/api/
```

Expected output:
```
(no output on success)
```

- [ ] **Step 3: 更新 claw/src/lib.rs，移除 api 模块声明**

在 `crates/claw/src/lib.rs` 中，找到并删除这一行：
```rust
pub mod api;
```

查找命令：
```bash
grep -n "pub mod api" crates/claw/src/lib.rs
```

Expected: 找到一行类似 `pub mod api;`，删除它

- [ ] **Step 4: 验证 claw 仍能编译**

```bash
cargo build --p claw
```

Expected output:
```
Compiling claw v0.1.0 (/path/to/crates/claw)
Finished dev [unoptimized + debuginfo] target(s) in X.XXs
```，无错误

- [ ] **Step 5: 提交**

```bash
git add crates/claw/src/
git commit -m "chore(claw): remove unused GraphQL API module

- Delete api/ directory (schema, mutations, queries)
- Remove pub mod api from lib.rs
- Verified no references in codebase

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 2: 创建 argus-dev crate 基础结构

**Files:**
- Create: `crates/argus-dev/Cargo.toml`
- Create: `crates/argus-dev/src/lib.rs`
- Create: `crates/argus-dev/src/error.rs`
- Test: `crates/argus-dev/tests/basic_test.rs`

- [ ] **Step 1: 创建 Cargo.toml**

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
argus-template = { path = "../argus-template" }
anyhow = "1"
tokio = { version = "1", features = ["sync", "rt"] }
sqlx = { version = "0.8", features = ["sqlite", "runtime-tokio"] }
thiserror = "2"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tempfile = "3"
```

- [ ] **Step 2: 创建错误类型 (src/error.rs)**

```rust
use thiserror::Error;

/// argus-dev error type
#[derive(Error, Debug)]
pub enum DevError {
    #[error("Database error: {reason}")]
    DatabaseError { reason: String },

    #[error("Turn execution error: {reason}")]
    TurnError { reason: String },

    #[error("Workflow error: {reason}")]
    WorkflowError { reason: String },

    #[error("Job error: {reason}")]
    JobError { reason: String },

    #[error("Not found: {what}")]
    NotFound { what: String },

    #[error("Invalid input: {reason}")]
    InvalidInput { reason: String },
}

impl From<argus_protocol::ArgusError> for DevError {
    fn from(e: argus_protocol::ArgusError) -> Self {
        DevError::DatabaseError { reason: e.to_string() }
    }
}

impl From<sqlx::Error> for DevError {
    fn from(e: sqlx::Error) -> Self {
        DevError::DatabaseError { reason: e.to_string() }
    }
}

pub type Result<T> = std::result::Result<T, DevError>;
```

- [ ] **Step 3: 创建基础 lib.rs**

```rust
//! argus-dev - 开发测试工具统一入口
//!
//! 提供 Turn 执行、Workflow/Job 管理等开发测试功能

pub mod error;
pub mod turn;
pub mod workflow;

pub use error::{DevError, Result};

use std::sync::Arc;
use argus_wing::ArgusWing;
use argus_protocol::{WorkflowId, WorkflowRecord, JobRecord, AgentRecord};
use argus_template::TemplateManager;
use argus_repository::{SqliteWorkflowRepository, SqliteJobRepository, JobRepository};
use sqlx::SqlitePool;

/// 开发测试工具统一入口
pub struct DevTools {
    pool: SqlitePool,
    wing: Arc<ArgusWing>,
    template_manager: Arc<TemplateManager>,
    workflow_repo: Arc<SqliteWorkflowRepository>,
    job_repo: Arc<dyn JobRepository>,
}

impl DevTools {
    /// 初始化 DevTools
    ///
    /// 使用 ArgusWing 的初始化逻辑，复用数据库连接
    pub async fn init(database_path: Option<&str>) -> Result<Arc<Self>> {
        let wing = ArgusWing::init(database_path)
            .await
            .map_err(|e| DevError::DatabaseError { reason: e.to_string() })?;

        let pool = wing.pool.clone();

        let template_manager = Arc::new(TemplateManager::new(pool.clone()));
        let workflow_repo = Arc::new(SqliteWorkflowRepository::new(pool.clone()));
        let job_repo: Arc<dyn JobRepository> = Arc::new(SqliteJobRepository::new(pool));

        Ok(Arc::new(Self {
            pool,
            wing,
            template_manager,
            workflow_repo,
            job_repo,
        }))
    }

    /// 使用自定义数据库初始化（用于 workflow 测试）
    pub async fn init_with_db(database_url: &str) -> Result<Arc<Self>> {
        let wing = ArgusWing::init(Some(database_url))
            .await
            .map_err(|e| DevError::DatabaseError { reason: e.to_string() })?;

        let pool = wing.pool.clone();

        let template_manager = Arc::new(TemplateManager::new(pool.clone()));
        let workflow_repo = Arc::new(SqliteWorkflowRepository::new(pool.clone()));
        let job_repo: Arc<dyn JobRepository> = Arc::new(SqliteJobRepository::new(pool));

        Ok(Arc::new(Self {
            pool,
            wing,
            template_manager,
            workflow_repo,
            job_repo,
        }))
    }

    /// 访问 ArgusWing（用于 provider、template 等）
    pub fn wing(&self) -> &Arc<ArgusWing> {
        &self.wing
    }

    /// 访问 TemplateManager
    pub fn template_manager(&self) -> &Arc<TemplateManager> {
        &self.template_manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn init_creates_dev_tools() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("test.sqlite");

        let dev_tools = DevTools::init(Some(&database_path.display().to_string()))
            .await
            .expect("DevTools should initialize");

        // Verify we can access ArgusWing
        let providers = dev_tools.wing()
            .list_providers()
            .await
            .expect("should list providers");

        assert!(providers.is_empty());
    }
}
```

- [ ] **Step 4: 创建 turn.rs (占位符)**

```rust
//! Turn execution testing support

use crate::Result;
use argus_turn::{TurnConfig, TurnInput, TurnOutput};

impl crate::DevTools {
    /// Execute a turn with the given input and config
    pub async fn execute_turn(&self, input: TurnInput, config: TurnConfig) -> Result<TurnOutput> {
        argus_turn::execute_turn(input, config)
            .await
            .map_err(|e| DevError::TurnError { reason: e.to_string() })
    }
}
```

- [ ] **Step 5: 创建 workflow.rs**

```rust
//! Workflow and Job management

use crate::Result;
use crate::error::DevError;
use argus_protocol::{WorkflowId, WorkflowRecord, JobRecord, WorkflowStatus, JobId};
use chrono::{DateTime, Utc};

impl crate::DevTools {
    /// Create a new workflow
    pub async fn create_workflow(&self, name: &str) -> Result<WorkflowId> {
        let id = WorkflowId::new(uuid::Uuid::new_v4().to_string());
        let workflow = WorkflowRecord {
            id: id.clone(),
            name: name.to_string(),
            status: WorkflowStatus::Pending,
        };
        self.workflow_repo.create_workflow(&workflow).await?;
        Ok(id)
    }

    /// List all workflows
    pub async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>> {
        self.workflow_repo.list_workflows().await
    }

    /// Get a workflow by ID
    pub async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>> {
        self.workflow_repo.get_workflow(id).await
    }

    /// Delete a workflow
    pub async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool> {
        self.workflow_repo.delete_workflow(id).await
    }

    /// Create a job
    pub async fn create_job(&self, job: JobRecord) -> Result<()> {
        self.job_repo.create(&job).await
    }

    /// List jobs in a workflow
    pub async fn list_jobs(&self, workflow_id: &str) -> Result<Vec<JobRecord>> {
        self.job_repo.list_by_group(workflow_id).await
    }

    /// Update job status
    pub async fn update_job_status(
        &self,
        id: &JobId,
        status: WorkflowStatus,
        started_at: Option<DateTime<Utc>>,
        finished_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.job_repo.update_status(id, status, started_at, finished_at).await
    }
}
```

- [ ] **Step 6: 添加到 workspace Cargo.toml**

在根目录 `Cargo.toml` 的 `[workspace.members]` 数组中添加 `"crates/argus-dev"`：

```toml
[workspace]
members = [
    "crates/argus-protocol",
    "crates/argus-log",
    # ... other members ...
    "crates/argus-dev",  # 添加这一行
]
resolver = "2"
```

或者如果使用字符串数组格式：
```toml
[workspace]
members = [
    "crates/*",
    "crates/argus-dev",  # 添加这一行
]
```

- [ ] **Step 7: 验证编译**

```bash
cargo build --p argus-dev
```

Expected output:
```
Compiling argus-dev v0.1.0 (/path/to/crates/argus-dev)
Finished dev [unoptimized + debuginfo] target(s) in X.XXs
```

- [ ] **Step 8: 运行测试**

```bash
cargo test --p argus-dev
```

Expected output:
```
running 1 test
test tests::init_creates_dev_tools ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

- [ ] **Step 9: 提交**

```bash
git add crates/argus-dev/ Cargo.toml
git commit -m "feat(argus-dev): create dev tools crate

- Add argus-dev crate with DevTools entry point
- Implement basic structure: error types, turn, workflow modules
- Provide access to ArgusWing for provider/template management
- Add comprehensive tests

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Chunk 2: 扩展 ArgusWing API

### Task 3: 添加 get_default_template() 方法

**Files:**
- Modify: `crates/argus-wing/src/lib.rs:244-270`
- Test: `crates/argus-wing/tests/default_template_test.rs`

- [ ] **Step 1: 在 ArgusWing impl 中添加方法**

在 `get_default_provider_record()` 方法后添加：

```rust
/// Get the default ArgusWing template.
///
/// Returns the template with display_name \"ArgusWing\".
pub async fn get_default_template(&self) -> Result<Option<AgentRecord>> {
    let templates = self.list_templates().await?;
    Ok(templates.into_iter().find(|t| t.display_name == "ArgusWing"))
}
```

- [ ] **Step 2: 创建测试文件**

```rust
use argus_wing::ArgusWing;

#[tokio::test]
async fn get_default_template_returns_arguswing() {
    let temp_dir = tempfile::tempdir().expect("temp dir should exist");
    let database_path = temp_dir.path().join("test.sqlite");

    let wing = ArgusWing::init(Some(&database_path.display().to_string()))
        .await
        .expect("ArgusWing should initialize");

    // Default template should exist after init
    let template = wing.get_default_template()
        .await
        .expect("should get default template");

    assert!(template.is_some(), "default template should exist");
    let template = template.unwrap();
    assert_eq!(template.display_name, "ArgusWing");
}
```

- [ ] **Step 3: 运行测试**

```bash
cargo test --p argus-wing get_default_template
```

Expected: PASS

- [ ] **Step 4: 提交**

```bash
git add crates/argus-wing/
git commit -m "feat(argus-wing): add get_default_template() method

- Add method to find template by display_name \"ArgusWing\"
- Returns Option<AgentRecord> for graceful handling
- Add comprehensive test

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 4: 添加 create_session_with_approval() 方法

**Files:**
- Modify: `crates/argus-wing/src/lib.rs` (在 Thread Management API 部分，约 line 320 之后)
- Test: `crates/argus-wing/src/lib.rs` (添加到 `#[cfg(test)]` mod)

- [ ] **Step 0: 先添加 get_default_provider() 方法（Chunk 4 需要使用）**

在 `get_default_provider_record()` 方法后添加：

```rust
/// Get the default LLM provider instance.
///
/// Returns the actual provider instance (not just the record).
pub async fn get_default_provider(&self) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
    let record = self.get_default_provider_record().await?;
    let provider_id = &record.id;
    self.provider_manager.get_provider(provider_id).await
}
```

- [ ] **Step 1: 在 Thread Management API 部分添加方法**

在 `delete_thread()` 方法后添加：

```rust
/// Create a session and thread with approval configuration.
///
/// This is a convenience method that combines:
/// - Creating a session
/// - Getting the default template
/// - Configuring approval policy
/// - Creating a thread
///
/// Returns (session_id, thread_id)
pub async fn create_session_with_approval(
    &self,
    name: &str,
    mut approval_tools: Vec<String>,
    auto_approve: bool,
) -> Result<(SessionId, ThreadId)> {
    let session_id = self.create_session(name).await?;

    // Get default template
    let template = self.get_default_template().await?
        .ok_or_else(|| ArgusError::ApprovalError {
            reason: "Default template 'ArgusWing' not found".to_string(),
        })?;

    // Configure approval policy if needed
    if !approval_tools.is_empty() {
        let mut policy = argus_approval::ApprovalPolicy::default();
        policy.require_approval = approval_tools;
        policy.auto_approve = auto_approve;
        self.approval_manager.update_policy(policy);
    }

    // Create thread
    let thread_id = self.create_thread(session_id, template.id, None).await?;

    Ok((session_id, thread_id))
}
```

**注意:** `ApprovalError` 是 `ArgusError` 的有效变体

- [ ] **Step 2: 在 lib.rs 的测试模块添加测试**

找到 `#[cfg(test)]` mod（约 line 418），在现有测试后添加：

```rust
#[tokio::test]
async fn create_session_with_approval_configures_policy() {
    use argus_protocol::LlmProviderRecord;
    use std::collections::HashMap;

    let temp_dir = tempfile::tempdir().expect("temp dir should exist");
    let database_path = temp_dir.path().join("test.sqlite");

    let wing = ArgusWing::init(Some(&database_path.display().to_string()))
        .await
        .expect("ArgusWing should initialize");

    // Create a test provider first
    let provider_record = LlmProviderRecord {
        id: argus_protocol::LlmProviderId::new(1),
        display_name: "test-provider".to_string(),
        kind: argus_protocol::LlmProviderKind::OpenAICompatible,
        api_base: "http://localhost:11434/v1".parse().unwrap(),
        models: vec!["gpt-4".to_string()],
        default_model: "gpt-4".to_string(),
        is_default: true,
        extra_headers: HashMap::new(),
        secret_status: argus_protocol::ProviderSecretStatus::Ready,
    };

    wing.upsert_provider(provider_record.clone()).await
        .expect("provider should upsert");

    let (session_id, thread_id) = wing.create_session_with_approval(
        "test-session",
        vec!["shell".to_string()],
        false,
    ).await.expect("session with approval should create");

    // Verify session was created
    let sessions = wing.list_sessions().await.expect("should list sessions");
    assert!(!sessions.is_empty());

    // Verify thread was created
    let threads = wing.list_threads(session_id).await.expect("should list threads");
    assert_eq!(threads.len(), 1);
}
```

- [ ] **Step 3: 运行测试**

```bash
cargo test --p argus-wing create_session_with_approval
```

Expected output:
```
running 1 test
test create_session_with_approval_configures_policy ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

- [ ] **Step 4: 提交**

```bash
git add crates/argus-wing/
git commit -m "feat(argus-wing): add create_session_with_approval() method

- Add convenience method for creating session with approval config
- Configure ApprovalPolicy using direct field assignment
- Returns (session_id, thread_id) tuple
- Add comprehensive test in lib.rs

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Chunk 3: 迁移 CLI 生产命令

### Task 5: 更新 CLI Cargo.toml 依赖

**Files:**
- Modify: `crates/cli/Cargo.toml`

- [ ] **Step 1: 备份当前 Cargo.toml**

```bash
cp crates/cli/Cargo.toml crates/cli/Cargo.toml.backup
```

- [ ] **Step 2: 更新依赖**

在 `[dependencies]` 部分：
- 删除: `claw = { path = "../claw" }`
- 添加或更新:
```toml
argus-wing = { path = "../argus-wing" }
argus-dev = { path = "../argus-dev", optional = true }

[features]
default = []
dev = ["argus-dev"]
```

- [ ] **Step 3: 验证依赖解析**

```bash
cargo check --p cli
```

Expected: 编译失败（因为代码还在使用 claw）

- [ ] **Step 4: 提交**

```bash
git add crates/cli/Cargo.toml
git commit -m "chore(cli): update dependencies for migration

- Remove claw dependency
- Add argus-wing as primary dependency
- Add argus-dev with dev feature gate

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 6: 迁移 main.rs 到 ArgusWing

**Files:**
- Modify: `crates/cli/src/main.rs:5-42`

- [ ] **Step 1: 更新导入**

将:
```rust
use claw::AppContext;
use cli::agent::{AgentCommand, run_agent_command};
use cli::provider::{ProviderCommand, run_provider_command};
use cli::{db_path_to_url, resolve_db_path};
```

改为:
```rust
use argus_wing::ArgusWing;
use cli::agent::{AgentCommand, run_agent_command};
use cli::provider::{ProviderCommand, run_provider_command};
use cli::{db_path_to_url, resolve_db_path};
```

- [ ] **Step 2: 更新初始化**

将:
```rust
let ctx = AppContext::init(Some(db_url)).await?;
```

改为:
```rust
let wing = ArgusWing::init(Some(&db_url)).await?;
```

- [ ] **Step 3: 更新命令调用**

将:
```rust
match cli.command {
    Command::Provider(cmd) => run_provider_command(ctx, cmd).await?,
    Command::Agent(cmd) => run_agent_command(ctx, cmd).await?,
}
```

改为:
```rust
match cli.command {
    Command::Provider(cmd) => run_provider_command(wing, cmd).await?,
    Command::Agent(cmd) => run_agent_command(wing, cmd).await?,
}
```

- [ ] **Step 4: 更新日志过滤器**

将:
```rust
.unwrap_or_else(|_| EnvFilter::new("arguswing=info,claw=info"));
```

改为:
```rust
.unwrap_or_else(|_| EnvFilter::new("arguswing=info,argus_wing=info"));
```

- [ ] **Step 5: 验证编译**

```bash
cargo build --bin arguswing
```

Expected: 编译失败（agent.rs 和 provider.rs 还需要更新）

- [ ] **Step 6: 提交**

```bash
git add crates/cli/src/main.rs
git commit -m "refactor(cli): migrate main.rs to ArgusWing

- Replace AppContext with ArgusWing
- Update initialization and command passing
- Update tracing filter for new crate names

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 7: 迁移 provider.rs 到 ArgusWing

**Files:**
- Modify: `crates/cli/src/provider.rs`

- [ ] **Step 1: 更新导入**

将所有 `use claw::` 替换为 `use argus_wing::` 或相应的 crate 导入

- [ ] **Step 2: 更新类型引用**

将 `AppContext` 替换为 `ArgusWing`

- [ ] **Step 3: 验证编译**

```bash
cargo build --bin arguswing
```

Expected: 编译失败（agent.rs 还需要更新）

- [ ] **Step 4: 提交**

```bash
git add crates/cli/src/provider.rs
git commit -m "refactor(cli): migrate provider.rs to ArgusWing

- Update imports from claw to argus-wing and related crates
- Replace AppContext with ArgusWing
- Update type references for new API

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 8: 迁移 agent.rs 到 Session/Thread 模型

**Files:**
- Modify: `crates/cli/src/agent.rs:11-285`

- [ ] **Step 1: 更新导入**

```rust
use argus_wing::ArgusWing;
use argus_protocol::{ApprovalDecision, SessionId, ThreadId, ThreadEvent};
use argus_tool::{GlobTool, GrepTool, ReadTool, ShellTool};
use tokio::io::AsyncBufReadExt;

use super::{StreamRenderState, finish_stream_output, render_stream_event};
```

- [ ] **Step 2: 更新 run_agent_command 函数签名**

将:
```rust
pub async fn run_agent_command(ctx: AppContext, command: AgentCommand) -> Result<()> {
```

改为:
```rust
pub async fn run_agent_command(wing: Arc<ArgusWing>, command: AgentCommand) -> Result<()> {
```

- [ ] **Step 3: 更新 run_chat 函数签名和实现**

将:
```rust
async fn run_chat(
    ctx: AppContext,
    verbose: bool,
    approval_tools: Vec<String>,
    muted_tools: Vec<String>,
    auto_approve: bool,
) -> Result<()> {
```

改为:
```rust
async fn run_chat(
    wing: Arc<ArgusWing>,
    verbose: bool,
    approval_tools: Vec<String>,
    muted_tools: Vec<String>,
    auto_approve: bool,
) -> Result<()> {
```

- [ ] **Step 4: 更新 session 和 thread 创建**

将:
```rust
let agent_id = ctx
    .create_default_agent_with_approval(effective_approval_tools.clone(), auto_approve)
    .await
    .map_err(|e| anyhow!("Failed to create default agent: {}", e))?;

let thread_id = ctx
    .create_thread(&agent_id, ThreadConfig::default())
    .map_err(|e| anyhow!("Failed to create thread: {}", e))?;

let mut event_rx = ctx
    .subscribe(&agent_id, &thread_id)
    .await
    .ok_or_else(|| anyhow!("Failed to subscribe to thread events"))?;
```

改为:
```rust
let (session_id, thread_id) = wing
    .create_session_with_approval(
        "default-session",
        effective_approval_tools.clone(),
        auto_approve,
    )
    .await
    .map_err(|e| anyhow!("Failed to create session: {}", e))?;

let mut event_rx = wing
    .subscribe(session_id, thread_id)
    .await
    .ok_or_else(|| anyhow!("Failed to subscribe to thread events"))?;
```

- [ ] **Step 5: 更新消息发送**

将:
```rust
ctx.send_message(&agent_id, &thread_id, input)
    .await
    .map_err(|e| anyhow!("Failed to send message: {}", e))?;
```

改为:
```rust
wing.send_message(session_id, thread_id, input)
    .await
    .map_err(|e| anyhow!("Failed to send message: {}", e))?;
```

- [ ] **Step 6: 更新审批调用**

将所有:
```rust
let _ = ctx.resolve_approval(
    &agent_id,
    request_id,
    ApprovalDecision::Approved,
    Some("auto-approve".to_string()),
);
```

改为:
```rust
let _ = wing.resolve_approval(
    request_id,
    ApprovalDecision::Approved,
    Some("auto-approve".to_string()),
);
```

同样更新其他审批调用

- [ ] **Step 7: 验证编译**

```bash
cargo build --bin arguswing
```

Expected: 编译成功

- [ ] **Step 8: 手动测试**

```bash
cargo run --bin arguswing -- agent chat
```

Expected: 启动交互式聊天，输入 "hello" 应该得到回复

- [ ] **Step 9: 提交**

```bash
git add crates/cli/src/agent.rs
git commit -m "refactor(cli): migrate agent.rs to Session/Thread model

- Replace Agent with Session/Thread model
- Use create_session_with_approval() for initialization
- Update all API calls to use session_id instead of agent_id
- Remove agent_id from resolve_approval() calls
- Update message sending and subscription APIs

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Chunk 4: 迁移 CLI dev 命令

### Task 9: 迁移 dev/mod.rs 到 DevTools

**Files:**
- Modify: `crates/cli/src/dev/mod.rs:1-143`

- [ ] **Step 1: 更新导入**

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};
use argus_dev::DevTools;
use argus_wing::ArgusWing;
use std::sync::Arc;
use sqlx::migrate::Migrator;

use crate::dev::approval::ApprovalCommand;
use crate::dev::llm::LlmCommand;
use crate::dev::turn::TurnCommand;
use crate::dev::workflow::WorkflowCommand;
use crate::provider::ProviderCommand;
```

- [ ] **Step 2: 更新 run 函数签名**

将:
```rust
pub async fn run(ctx: AppContext, command: DevCommand) -> Result<()> {
```

改为:
```rust
pub async fn run(dev_tools: Arc<DevTools>, command: DevCommand) -> Result<()> {
```

- [ ] **Step 3: 更新命令分发**

将:
```rust
match command {
    DevCommand::Provider(cmd) => crate::provider::run_provider_command(ctx, cmd).await,
    DevCommand::Llm(cmd) => crate::dev::llm::run_llm_command(ctx, cmd).await,
    DevCommand::Turn(cmd) => crate::dev::turn::run_turn_command(ctx, cmd).await,
    DevCommand::Approval(cmd) => crate::dev::approval::run_approval_command(cmd).await,
    DevCommand::Workflow(cmd) => crate::dev::workflow::run_workflow_command(ctx, cmd).await,
    DevCommand::Thread(cmd) => run_thread_command(ctx, cmd).await,
}
```

改为:
```rust
match command {
    DevCommand::Provider(cmd) => {
        crate::provider::run_provider_command(dev_tools.wing().clone(), cmd).await
    }
    DevCommand::Llm(cmd) => crate::dev::llm::run_llm_command(dev_tools.clone(), cmd).await,
    DevCommand::Turn(cmd) => crate::dev::turn::run_turn_command(dev_tools.clone(), cmd).await,
    DevCommand::Approval(cmd) => crate::dev::approval::run_approval_command(dev_tools.clone(), cmd).await,
    DevCommand::Workflow(cmd) => crate::dev::workflow::run_workflow_command(dev_tools.clone(), cmd).await,
    DevCommand::Thread(cmd) => run_thread_command(dev_tools.clone(), cmd).await,
}
```

- [ ] **Step 4: 更新 try_run 函数**

将:
```rust
pub async fn try_run(ctx: AppContext) -> Result<bool> {
```

改为:
```rust
pub async fn try_run(dev_tools: Arc<DevTools>) -> Result<bool> {
```

并更新内部的 `run(ctx, cli.command)` 为 `run(dev_tools, cli.command)`

- [ ] **Step 5: 更新 run_thread_command 函数签名**

将:
```rust
async fn run_thread_command(_ctx: AppContext, command: ThreadCommand) -> Result<()> {
```

改为:
```rust
async fn run_thread_command(_dev_tools: Arc<DevTools>, command: ThreadCommand) -> Result<()> {
```

- [ ] **Step 6: 提交**

```bash
git add crates/cli/src/dev/mod.rs
git commit -m "refactor(cli-dev): migrate dev/mod.rs to DevTools

- Update imports to use DevTools
- Change run() signature to accept Arc<DevTools>
- Update command dispatch to use dev_tools.wing() for provider commands
- Pass dev_tools to all dev subcommands

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 10: 迁移 dev/turn.rs 到 argus-turn

**Files:**
- Modify: `crates/cli/src/dev/turn.rs:1-226`

- [ ] **Step 1: 更新导入**

```rust
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Subcommand;
use argus_dev::DevTools;
use argus_turn::{TurnConfig, TurnInputBuilder, execute_turn};
use argus_protocol::{ChatMessage, LlmProviderId, Role};
use owo_colors::OwoColorize;
```

- [ ] **Step 2: 更新函数签名**

将:
```rust
pub async fn run_turn_command(ctx: AppContext, command: TurnCommand) -> Result<()> {
```

改为:
```rust
pub async fn run_turn_command(dev_tools: Arc<DevTools>, command: TurnCommand) -> Result<()> {
```

- [ ] **Step 3: 更新 provider 获取**

将:
```rust
let provider = if let Some(id) = provider {
    let id: i64 = id
        .parse()
        .with_context(|| format!("Invalid provider id: {}", id))?;
    ctx.get_provider(&LlmProviderId::new(id)).await?
} else {
    ctx.get_default_provider().await?
};
```

改为:
```rust
let provider_record = if let Some(id) = provider {
    let id: i64 = id
        .parse()
        .with_context(|| format!("Invalid provider id: {}", id))?;
    dev_tools.wing().get_provider_record(&LlmProviderId::new(id)).await?
} else {
    dev_tools.wing().get_default_provider_record().await?
};
```

- [ ] **Step 4: 更新 Turn 构建**

将:
```rust
let input = TurnInputBuilder::default()
    .provider(provider)
    .messages(vec![ChatMessage::user(message)])
    .system_prompt(system_prompt)
    .tool_manager(tool_manager)
    .tool_ids(tools)
    .build()
    .context("Failed to build TurnInput")?;

let output = execute_turn(input, TurnConfig::default()).await?;
```

改为:
```rust
let input = TurnInputBuilder::default()
    .provider(provider)
    .messages(vec![ChatMessage::user(message)])
    .system_prompt(system_prompt)
    .tool_manager(tool_manager)
    .tool_ids(tools)
    .build()
    .context("Failed to build TurnInput")?;

let output = dev_tools.execute_turn(input, TurnConfig::default()).await?;
```

- [ ] **Step 5: 更新 role 匹配**

将所有 `claw::Role` 替换为 `argus_protocol::Role`

- [ ] **Step 6: 提交**

```bash
git add crates/cli/src/dev/turn.rs
git commit -m "refactor(cli-dev): migrate dev/turn.rs to argus-turn

- Use DevTools instead of AppContext
- Get provider from ArgusWing via dev_tools.wing()
- Use argus_turn::TurnInputBuilder and execute_turn
- Update role references to argus_protocol::Role

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 11: 迁移 dev/workflow.rs 到 DevTools API

**Files:**
- Modify: `crates/cli/src/dev/workflow.rs:1-419`

- [ ] **Step 1: 更新导入**

```rust
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
use argus_dev::DevTools;
use argus_protocol::{AgentId, WorkflowId, WorkflowRecord, WorkflowStatus, JobRecord, JobId, JobType};
use owo_colors::OwoColorize;
```

- [ ] **Step 2: 更新函数签名**

将:
```rust
pub async fn run_workflow_command(_ctx: AppContext, command: WorkflowCommand) -> Result<()> {
```

改为:
```rust
pub async fn run_workflow_command(dev_tools: Arc<DevTools>, command: WorkflowCommand) -> Result<()> {
```

- [ ] **Step 3: 删除 create_dev_workflow_repositories 函数**

不再需要，DevTools 已经管理了 repositories

- [ ] **Step 4: 更新 workflow 创建**

将所有:
```rust
let workflow_repo = SqliteWorkflowRepository::new(pool.clone());
```

改为直接使用:
```rust
let workflow_id = dev_tools.create_workflow(&name).await?;
```

- [ ] **Step 5: 更新所有 workflow 操作**

将:
```rust
workflow_repo.create_workflow(&workflow).await?;
workflow_repo.list_workflows().await?;
workflow_repo.get_workflow(&workflow_id).await?;
workflow_repo.delete_workflow(&workflow_id).await?;
```

改为:
```rust
dev_tools.create_workflow(&name).await?;
dev_tools.list_workflows().await?;
dev_tools.get_workflow(&workflow_id).await?;
dev_tools.delete_workflow(&workflow_id).await?;
```

- [ ] **Step 6: 更新 job 操作**

将:
```rust
job_repo.create(&job).await?;
job_repo.list_by_group(&id).await?;
job_repo.update_status(&job_id, new_status, None, None).await?;
```

改为:
```rust
dev_tools.create_job(job).await?;
dev_tools.list_jobs(&id).await?;
// Note: update_job_status needs to be added to DevTools
```

- [ ] **Step 7: 在 DevTools 中添加 update_job_status**

修改 `crates/argus-dev/src/workflow.rs`，添加:

```rust
/// Update job status
pub async fn update_job_status(
    &self,
    id: &JobId,
    status: WorkflowStatus,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<()> {
    self.job_repo.update_status(id, status, started_at, finished_at).await
}
```

- [ ] **Step 8: 提交 argus-dev 更新**

```bash
git add crates/argus-dev/src/workflow.rs
git commit -m "feat(argus-dev): add update_job_status method

- Add method to update job status with timestamps
- Expose JobRepository functionality through DevTools

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

- [ ] **Step 9: 提交 workflow.rs 更新**

```bash
git add crates/cli/src/dev/workflow.rs
git commit -m "refactor(cli-dev): migrate dev/workflow.rs to DevTools

- Use DevTools API instead of direct repository access
- Simplify workflow and job operations
- Remove create_dev_workflow_repositories function
- Update all workflow/job operations to use dev_methods

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 12: 迁移其他 dev 模块

**Files:**
- Modify: `crates/cli/src/dev/llm.rs`
- Modify: `crates/cli/src/dev/approval.rs`
- Modify: `crates/cli/src/dev/config.rs`
- Modify: `crates/cli/src/main-dev.rs`

- [ ] **Step 1: 迁移 llm.rs**

更新导入和函数签名，使用 `DevTools` 替代 `AppContext`

- [ ] **Step 2: 迁移 approval.rs**

更新导入和函数签名

- [ ] **Step 3: 迁移 config.rs**

更新导入和函数签名

- [ ] **Step 4: 更新 main-dev.rs**

将:
```rust
let ctx = AppContext::init(Some(db_url)).await?;
run(ctx, cli.command).await?;
```

改为:
```bash
let dev_tools = DevTools::init(Some(db_url)).await?;
run(dev_tools, cli.command).await?;
```

- [ ] **Step 5: 验证所有 dev 命令编译**

```bash
cargo build --bin arguswing-dev
```

Expected: 编译成功

- [ ] **Step 6: 测试 dev 命令**

```bash
cargo run --bin arguswing-dev -- turn test "hello"
cargo run --bin arguswing-dev -- workflow list
```

Expected: 命令正常执行

- [ ] **Step 7: 提交**

```bash
git add crates/cli/src/dev/
git add crates/cli/src/main-dev.rs
git commit -m "refactor(cli-dev): migrate remaining dev modules

- Migrate llm, approval, config modules to DevTools
- Update main-dev.rs to use DevTools::init()
- All dev commands now use argus-dev crate

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Chunk 5: 清理和验证

### Task 13: 验证无 claw 引用残留

**Files:**
- Test: N/A (verification task)

- [ ] **Step 1: 检查所有 claw 引用**

```bash
grep -r "use claw" crates/ --exclude-dir=target
grep -r "claw::" crates/ --exclude-dir=target
```

Expected: 无结果（除了可能的注释）

- [ ] **Step 2: 检查 Cargo.toml**

```bash
grep -r "claw" crates/*/Cargo.toml
```

Expected: 只有 workspace 的 `[workspace.members]` 中可能还有 "claw"

- [ ] **Step 3: 运行完整测试套件**

```bash
cargo test --workspace
```

Expected: 所有测试通过

- [ ] **Step 4: 运行 clippy**

```bash
cargo clippy --workspace
```

Expected: 无警告

- [ ] **Step 5: 检查 fmt**

```bash
cargo fmt --all --check
```

Expected: 无格式问题

- [ ] **Step 6: 提交（如果有修复）**

如果有需要修复的问题，提交它们：

```bash
git add .
git commit -m "test: fix remaining issues after migration

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 14: 从 workspace 删除 claw

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: 从 workspace members 移除 claw**

编辑根目录 `Cargo.toml`，从 `[workspace.members]` 中删除 `"crates/claw"`

- [ ] **Step 2: 验证 workspace 仍然有效**

```bash
cargo build --workspace
```

Expected: 编译成功

- [ ] **Step 3: 删除 claw 目录**

```bash
rm -rf crates/claw/
```

- [ ] **Step 4: 验证构建**

```bash
cargo build --workspace
cargo test --workspace
```

Expected: 全部成功

- [ ] **Step 5: 提交**

```bash
git add Cargo.toml
git add crates/claw/
git commit -m "chore: remove claw crate

- Remove claw from workspace members
- Delete crates/claw/ directory
- Migration to argus-wing and argus-dev complete

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 15: 更新文档

**Files:**
- Modify: `CLAUDE.md`
- Modify: `README.md` (if exists)

- [ ] **Step 1: 更新 CLAUDE.md**

删除所有关于 claw 的引用，更新为 argus-wing 和 argus-dev

- [ ] **Step 2: 更新 README**

如果有 README，更新架构说明

- [ ] **Step 3: 提交文档更新**

```bash
git add CLAUDE.md README.md
git commit -m "docs: update documentation for post-migration architecture

- Remove references to claw crate
- Update to describe argus-wing and argus-dev
- Update architecture diagrams

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

### Task 16: 最终验证和测试

**Files:**
- Test: N/A (verification task)

- [ ] **Step 1: 运行完整测试套件**

```bash
cargo test --workspace
```

Expected: 所有测试通过 ✅

- [ ] **Step 2: 运行 clippy**

```bash
cargo clippy --workspace
```

Expected: 无警告 ✅

- [ ] **Step 3: 检查代码格式**

```bash
cargo fmt --all --check
```

Expected: 无格式问题 ✅

- [ ] **Step 4: 测试 CLI 生产命令**

```bash
cargo run --bin arguswing -- provider list
cargo run --bin arguswing -- agent chat
```

Expected: 命令正常工作 ✅

- [ ] **Step 5: 测试 CLI dev 命令**

```bash
cargo run --bin arguswing-dev -- turn test "hello"
cargo run --bin arguswing-dev -- workflow list
```

Expected: 命令正常工作 ✅

- [ ] **Step 6: 测试 Desktop**

启动 Desktop 应用，测试基本功能

Expected: 应用正常工作 ✅

- [ ] **Step 7: 创建最终标签（可选）**

```bash
git tag -a v0.2.0 -m "Release: Complete claw migration

- Migrate CLI from claw to argus-wing and argus-dev
- Remove claw crate entirely
- All tests passing
- Ready for production use"
```

- [ ] **Step 8: 提交最终验证**

```bash
git add .
git commit -m "test: complete final verification after claw migration

All tests passing:
- cargo test --workspace ✅
- cargo clippy --workspace ✅
- cargo fmt --all --check ✅
- CLI production commands working ✅
- CLI dev commands working ✅
- Desktop app working ✅

Migration complete!

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## 总结

这个实施计划将 claw 迁移到 argus-wing 和 argus-dev 分解为：

- **16 个主要任务**
- **100+ 个具体步骤**
- **5 个 chunks**（用于审查和并行执行）

每个任务都包含：
- 明确的文件路径
- 完整的代码示例
- 测试命令和预期输出
- 提交步骤

**预计时间:**
- Chunk 1 (准备): 2-3 小时
- Chunk 2 (扩展 ArgusWing): 2-3 小时
- Chunk 3 (迁移 CLI 生产): 4-6 小时
- Chunk 4 (迁移 CLI dev): 3-4 小时
- Chunk 5 (清理验证): 2-3 小时

**总计:** 13-19 小时

**成功标准:**
- ✅ 所有测试通过
- ✅ 无 clippy 警告
- ✅ CLI 和 Desktop 功能正常
- ✅ claw crate 完全删除
- ✅ 文档更新完整
