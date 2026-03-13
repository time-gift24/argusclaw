# Dispatch Tool 设计文档

## 概述

设计一个 `dispatch_agent` 工具，让主 Agent 可以派发任务给独立的 subagent 执行。支持两种模式：

1. **内存模式（默认）**：同步阻塞，主 Agent 等待 subagent 完成后继续
2. **编排模式**：主 Agent 派发后不再跟踪，用户可继续与主 Agent 对话

## 核心需求

- 同步阻塞式：主 Agent 等待 subagent 完成
- Subagent 自摘要：返回精简结果，避免上下文爆炸
- 编排模式：需要 LLM 提议 + 用户确认
- 进度提醒：通过 ThreadEvent 广播
- 调度器适配：内存优先，可选持久化

## 架构设计

### Job 执行后端抽象

```rust
/// Job 执行后端 trait
#[async_trait]
pub trait JobBackend: Send + Sync {
    /// 提交 job 到后端，返回 job_id
    async fn submit(&self, job: JobRequest) -> Result<JobId, JobError>;

    /// 等待 job 完成（用于同步模式）
    async fn wait(&self, job_id: &JobId) -> Result<JobResult, JobError>;

    /// 取消 job
    async fn cancel(&self, job_id: &JobId) -> Result<(), JobError>;

    /// 查询 job 状态
    async fn status(&self, job_id: &JobId) -> Result<JobStatus, JobError>;
}

/// Job 请求（后端无关）
pub struct JobRequest {
    pub agent_id: AgentId,
    pub prompt: String,
    pub context: Option<String>,
    pub timeout_secs: u64,
    pub backend: JobBackendKind,
}

#[derive(Clone, Copy)]
pub enum JobBackendKind {
    InMemory,
    Persistent,
}
```

### 两种后端实现

**InMemoryJobBackend**：
- 使用 `DashMap<JobId, InMemoryJob>` 存储运行中的 job
- `submit()` 直接 spawn tokio task 执行
- `wait()` 通过 oneshot channel 等待结果

**PersistentJobBackend**：
- 包装现有的 `JobRepository` + `Scheduler`
- `submit()` 写入数据库，由 Scheduler 调度
- `wait()` 订阅状态变更事件

### JobBackendKind 与 JobType 的关系

这两个概念是正交的：

- **JobBackendKind**：决定 job 存储和执行的位置（内存 vs 数据库）
- **JobType**（现有）：描述 job 的业务类型（Standalone/Workflow/Cron）

关系矩阵：
| JobBackendKind | 适用 JobType | 说明 |
|----------------|--------------|------|
| InMemory | Standalone | 临时性同步等待任务 |
| Persistent | Standalone/Workflow/Cron | 需要持久化、重试、定时执行的任务 |

### 数据结构

```rust
pub struct InMemoryJob {
    pub request: JobRequest,
    pub status: JobStatus,
    pub result: Option<JobResult>,
    pub started_at: Instant,
    pub result_tx: Option<oneshot::Sender<Result<JobResult, JobError>>>,
}

#[derive(Clone)]
pub struct InMemoryBackendConfig {
    pub default_timeout_secs: u64,
    pub progress_notify_interval_secs: u64,
    pub max_concurrent_jobs: usize,
}

pub struct JobResult {
    pub summary: String,
    pub token_usage: TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}
```

## DispatchTool 设计

### 工具定义

```rust
pub struct DispatchTool {
    job_backend: Arc<dyn JobBackend>,
    agent_manager: Arc<AgentManager>,
    approval_manager: Arc<ApprovalManager>,
    thread_event_sender: broadcast::Sender<ThreadEvent>,
    config: DispatchToolConfig,
}

impl DispatchTool {
    /// 获取 subagent 模板 ID
    fn subagent_template_id(&self) -> &AgentId {
        &self.config.subagent_template_id
    }
}

impl NamedTool for DispatchTool {
    fn name(&self) -> &str {
        "dispatch_agent"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "dispatch_agent".to_string(),
            description: include_str!("dispatch_prompt.md").to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "清晰描述 subagent 需要完成的任务"
                    },
                    "context": {
                        "type": "string",
                        "description": "可选：传递给 subagent 的上下文信息"
                    },
                    "summary_hint": {
                        "type": "string",
                        "description": "可选：指导 subagent 如何总结结果"
                    },
                    "orchestrate": {
                        "type": "boolean",
                        "description": "设为 true 表示编排模式（派发后主 agent 不再跟踪）"
                    }
                },
                "required": ["task"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }
}
```

### 工具描述词 (dispatch_prompt.md)

派发一个子任务给独立的 subagent 执行。

使用场景：
- 任务边界清晰，可以并行执行
- 需要 subagent 自主规划和执行多步骤

参数说明：
- task: 清晰描述任务目标和期望产出
- context: 传递必要的背景信息（如相关文件路径、已知约束）
- summary_hint: 指导 subagent 如何总结结果，例如"返回关键发现列表"
- orchestrate: 编排模式，用于工作流最后一步，主 agent 将不再跟踪

注意事项：
1. task 描述要具体，避免模糊指令
2. 合理使用 summary_hint 控制返回结果大小
3. 编排模式需要用户确认

示例：
{"task": "分析 src/auth 模块的安全风险", "summary_hint": "返回风险列表，每项不超过 50 字"}

### 执行逻辑

```rust
impl DispatchTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let task = args["task"].as_str().required("task")?.to_string();
        let context = args["context"].as_str().map(String::from);
        let summary_hint = args["summary_hint"].as_str().map(String::from);
        let orchestrate = args["orchestrate"].as_bool().unwrap_or(false);
        let thread_id = args["thread_id"].as_str()
            .map(ThreadId::new)
            .ok_or_else(|| ToolError::MissingParameter("thread_id"))?;

        if orchestrate {
            return self.handle_orchestrate_mode(thread_id, task, context, summary_hint).await;
        }

        self.dispatch_and_wait(thread_id, task, context, summary_hint).await
    }

    async fn dispatch_and_wait(
        &self,
        thread_id: ThreadId,
        task: String,
        context: Option<String>,
        summary_hint: Option<String>,
    ) -> Result<Value, ToolError> {
        let job = JobRequest {
            agent_id: self.config.subagent_template_id.clone(),
            prompt: self.build_prompt(task, context, summary_hint),
            timeout_secs: self.config.default_timeout_secs,
            backend: JobBackendKind::InMemory,
        };

        let job_id = self.job_backend.submit(job).await?;

        let _ = self.thread_event_sender.send(ThreadEvent::WaitingForSubagent {
            thread_id: thread_id.clone(),
            job_id: job_id.clone(),
            message: "等待 subagent 完成...".into(),
        });

        let notifier = self.spawn_progress_notifier(thread_id.clone(), job_id.clone());

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.default_timeout_secs),
            self.job_backend.wait(&job_id),
        ).await;

        notifier.abort();

        match result {
            Ok(Ok(job_result)) => {
                let _ = self.thread_event_sender.send(ThreadEvent::SubagentCompleted {
                    thread_id,
                    job_id,
                    summary: job_result.summary.clone(),
                });
                Ok(json!({
                    "success": true,
                    "summary": job_result.summary,
                    "tokens": job_result.token_usage.total_tokens,
                }))
            }
            Ok(Err(e)) => {
                let _ = self.thread_event_sender.send(ThreadEvent::SubagentFailed {
                    thread_id,
                    job_id,
                    error: e.to_string(),
                });
                Err(ToolError::ExecutionFailed {
                    tool_name: "dispatch_agent".into(),
                    reason: e.to_string(),
                })
            }
            Err(_) => {
                let _ = self.job_backend.cancel(&job_id).await;
                let _ = self.thread_event_sender.send(ThreadEvent::SubagentTimedOut {
                    thread_id,
                    job_id,
                    timeout_secs: self.config.default_timeout_secs,
                });
                Err(ToolError::ExecutionFailed {
                    tool_name: "dispatch_agent".into(),
                    reason: "Subagent 执行超时".into(),
                })
            }
        }
    }

    async fn handle_orchestrate_mode(
        &self,
        task: String,
        context: Option<String>,
        summary_hint: Option<String>,
    ) -> Result<Value, ToolError> {
        let job = JobRequest {
            agent_id: self.subagent_template_id(),
            prompt: self.build_prompt(task.clone(), context, summary_hint),
            timeout_secs: 3600,
            backend: JobBackendKind::Persistent,
        };

        let job_id = self.job_backend.submit(job).await?;

        let _ = self.thread_event_sender.send(ThreadEvent::OrchestratedJobDispatched {
            job_id,
            task,
            message: "编排任务已派发，主 agent 不再跟踪。您可以继续对话。".into(),
        });

        Ok(json!({
            "status": "dispatched",
            "job_id": job_id.as_ref(),
            "message": "任务已派发到后台执行，完成后会通知您"
        }))
    }

    fn build_prompt(
        &self,
        task: String,
        context: Option<String>,
        summary_hint: Option<String>,
    ) -> String {
        let mut prompt = String::new();

        prompt.push_str("## 输出要求（重要）\n\n");
        prompt.push_str("你的回复将直接返回给派发任务的主 agent。\n");
        prompt.push_str("请确保输出简洁、结构化，默认不超过 500 字。\n\n");

        if let Some(ctx) = context {
            prompt.push_str("## 上下文\n\n");
            prompt.push_str(&ctx);
            prompt.push_str("\n\n");
        }

        prompt.push_str("## 任务\n\n");
        prompt.push_str(&task);
        prompt.push_str("\n\n");

        if let Some(hint) = summary_hint {
            prompt.push_str("## 输出格式要求\n\n");
            prompt.push_str(&hint);
            prompt.push_str("\n\n");
        }

        prompt
    }
}
```

## ThreadEvent 扩展

为保持与现有 ThreadEvent 的一致性，新增事件也包含 `thread_id`：

```rust
pub enum ThreadEvent {
    // === 现有事件 ===
    // Processing { thread_id: ThreadId, turn_number: u32, event: LlmStreamEvent },
    // TurnCompleted { thread_id: ThreadId, turn_number: u32, token_usage: TokenUsage },

    // === Subagent 相关事件 ===
    WaitingForSubagent {
        thread_id: ThreadId,  // 主 agent 的 thread_id
        job_id: JobId,
        message: String,
    },

    SubagentProgress {
        thread_id: ThreadId,
        job_id: JobId,
        elapsed_secs: u64,
        message: String,
    },

    SubagentCompleted {
        thread_id: ThreadId,
        job_id: JobId,
        summary: String,
    },

    SubagentFailed {
        thread_id: ThreadId,
        job_id: JobId,
        error: String,
    },

    SubagentTimedOut {
        thread_id: ThreadId,
        job_id: JobId,
        timeout_secs: u64,
    },

    OrchestratedJobDispatched {
        thread_id: ThreadId,
        job_id: JobId,
        task: String,
        message: String,
    },

    OrchestrationConfirmationRequired {
        thread_id: ThreadId,
        confirmation_id: String,
        task: String,
        message: String,
    },
}
```

## Scheduler 适配

```rust
pub struct Scheduler {
    config: SchedulerConfig,
    persistent_backend: Arc<PersistentJobBackend>,
    in_memory_backend: Arc<InMemoryJobBackend>,
    agent_manager: Arc<AgentManager>,
    running_jobs: DashMap<JobId, JoinHandle<()>>,
    shutdown: CancellationToken,
}

impl Scheduler {
    pub async fn submit(&self, request: JobRequest) -> Result<JobId, SchedulerError> {
        match request.backend {
            JobBackendKind::InMemory => {
                self.in_memory_backend.submit(request).await
                    .map_err(|e| SchedulerError::SubmitFailed(e.to_string()))
            }
            JobBackendKind::Persistent => {
                self.persistent_backend.submit(request).await
                    .map_err(|e| SchedulerError::SubmitFailed(e.to_string()))
            }
        }
    }
}
```

## 进度提醒机制

```rust
impl DispatchTool {
    fn spawn_progress_notifier(
        &self,
        thread_id: ThreadId,
        job_id: JobId,
    ) -> JoinHandle<()> {
        let event_sender = self.thread_event_sender.clone();
        let interval_secs = self.config.progress_notify_interval_secs;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            let start = Instant::now();

            loop {
                interval.tick().await;

                let elapsed = start.elapsed().as_secs();
                let message = format!(
                    "Subagent 已执行 {} 分钟，请耐心等待...",
                    elapsed / 60
                );

                let _ = event_sender.send(ThreadEvent::SubagentProgress {
                    thread_id: thread_id.clone(),
                    job_id: job_id.clone(),
                    elapsed_secs: elapsed,
                    message,
                });
            }
        })
    }
}
```

## 错误处理

```rust
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("Job not found: {0}")]
    NotFound(JobId),

    #[error("Agent not found: {0}")]
    AgentNotFound(AgentId),

    #[error("Agent creation failed")]
    AgentCreationFailed,

    #[error("Thread not found")]
    ThreadNotFound,

    #[error("Job execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Job timed out")]
    Timeout,

    #[error("Job was cancelled")]
    Cancelled,

    #[error("Channel closed unexpectedly")]
    ChannelClosed,

    #[error("Too many concurrent jobs (max: {0})")]
    ConcurrencyLimit(usize),

    #[error("Repository error: {0}")]
    Repository(#[from] sqlx::Error),
}
```

## System Prompt 集成

### Agent System Prompt 片段

```markdown
## 任务派发工具使用指南

你可以使用 `dispatch_agent` 工具派发子任务给独立的 subagent 执行。

### 何时使用

- 任务边界清晰，可以并行执行
- 需要 subagent 自主规划多步骤操作

### 如何使用

1. **明确任务目标**：`task` 参数要具体，避免模糊指令
2. **传递上下文**：通过 `context` 传递必要的背景信息
3. **控制结果大小**：使用 `summary_hint` 指导 subagent 如何总结

### 编排模式

当你确定任务完成后需要生成最终报告时，可以使用编排模式：
```json
{
  "task": "根据前面所有调研结果，生成项目迁移方案文档",
  "orchestrate": true
}
```

编排模式会请求用户确认，确认后你将不再跟踪该任务，用户可以继续与你对话。
```

## 文件结构

```
crates/claw/src/
├── job/
│   ├── mod.rs              # 重导出
│   ├── types.rs            # JobRequest, JobResult, JobStatus, JobBackendKind
│   ├── backend.rs          # JobBackend trait
│   ├── memory.rs           # InMemoryJobBackend
│   ├── persistent.rs       # PersistentJobBackend
│   ├── repository.rs       # JobRepository trait (现有)
│   └── error.rs            # JobError (扩展)
│
├── scheduler/
│   ├── mod.rs
│   ├── config.rs           # SchedulerConfig (现有)
│   ├── error.rs            # SchedulerError (现有)
│   └── scheduler.rs        # 重构为使用 JobBackend
│
├── tool/
│   ├── mod.rs              # 注册 DispatchTool
│   ├── dispatch.rs         # DispatchTool 实现
│   └── ... (现有工具)
│
├── agents/
│   ├── thread/
│   │   └── types.rs        # ThreadEvent 扩展
│   └── ...
│
└── lib.rs                  # 导出新增类型
```

## 改动总结

| 模块 | 改动类型 | 说明 |
|------|----------|------|
| `JobBackend` trait | 新增 | 统一内存/持久化后端接口 |
| `InMemoryJobBackend` | 新增 | 管理内存中的 job |
| `PersistentJobBackend` | 新增 | 包装现有 JobRepository |
| `Scheduler` | 重构 | 使用 JobBackend 而非直接依赖 JobRepository |
| `DispatchTool` | 新增 | 派发 subagent 任务的工具 |
| `ThreadEvent` | 扩展 | 增加 subagent 相关事件 |
| `JobError` | 扩展 | 增加新错误类型 |

## 附录 A：InMemoryJobBackend 完整实现

```rust
pub struct InMemoryJobBackend {
    jobs: DashMap<JobId, InMemoryJob>,
    agent_manager: Arc<AgentManager>,
    config: InMemoryBackendConfig,
    event_sender: broadcast::Sender<JobEvent>,
}

#[derive(Clone)]
pub struct InMemoryJob {
    pub request: JobRequest,
    pub status: Arc<RwLock<JobStatus>>,
    pub result_rx: Mutex<Option<oneshot::Receiver<Result<JobResult, JobError>>>>,
    pub result_tx: Mutex<Option<oneshot::Sender<Result<JobResult, JobError>>>>,
    pub cancel_token: CancellationToken,
}

#[async_trait]
impl JobBackend for InMemoryJobBackend {
    async fn submit(&self, job: JobRequest) -> Result<JobId, JobError> {
        // 并发限制检查
        let running = self.jobs.iter()
            .filter(|e| *e.status.read().await == JobStatus::Running)
            .count();
        if running >= self.config.max_concurrent_jobs {
            return Err(JobError::ConcurrencyLimit(self.config.max_concurrent_jobs));
        }

        let job_id = JobId::new(Uuid::new_v4().to_string());
        let (result_tx, result_rx) = oneshot::channel();
        let cancel_token = CancellationToken::new();

        let in_memory_job = InMemoryJob {
            request: job.clone(),
            status: Arc::new(RwLock::new(JobStatus::Running)),
            result_rx: Mutex::new(Some(result_rx)),
            result_tx: Mutex::new(Some(result_tx)),
            cancel_token: cancel_token.clone(),
        };

        self.jobs.insert(job_id.clone(), in_memory_job);

        // Spawn 执行任务
        let jobs = self.jobs.clone();
        let agent_manager = self.agent_manager.clone();
        let event_sender = self.event_sender.clone();
        let job_id_clone = job_id.clone();

        tokio::spawn(async move {
            let result = Self::execute_job(
                &agent_manager,
                &job,
                cancel_token,
            ).await;

            // 更新状态
            if let Some(job_entry) = jobs.get(&job_id_clone) {
                let mut status = job_entry.status.write().await;
                *status = match &result {
                    Ok(_) => JobStatus::Succeeded,
                    Err(JobError::Cancelled) => JobStatus::Cancelled,
                    Err(JobError::Timeout) => JobStatus::TimedOut,
                    Err(_) => JobStatus::Failed,
                };
            }

            // 发送结果
            if let Some(job_entry) = jobs.get(&job_id_clone) {
                let mut tx = job_entry.result_tx.lock().await;
                if let Some(tx) = tx.take() {
                    let _ = tx.send(result);
                }
            }

            // 广播事件
            let _ = event_sender.send(JobEvent::Completed {
                job_id: job_id_clone,
                result: result.map(|r| r.summary),
            });
        });

        Ok(job_id)
    }

    async fn wait(&self, job_id: &JobId) -> Result<JobResult, JobError> {
        // 获取 result_rx 的所有权
        let rx = {
            let entry = self.jobs.get(job_id)
                .ok_or_else(|| JobError::NotFound(job_id.clone()))?;
            let mut rx = entry.result_rx.lock().await;
            rx.take().ok_or(JobError::AlreadyConsumed)?
        };

        // 等待结果
        rx.await.map_err(|_| JobError::ChannelClosed)?
    }

    async fn cancel(&self, job_id: &JobId) -> Result<(), JobError> {
        let entry = self.jobs.get(job_id)
            .ok_or_else(|| JobError::NotFound(job_id.clone()))?;

        entry.cancel_token.cancel();

        let mut status = entry.status.write().await;
        *status = JobStatus::Cancelled;

        Ok(())
    }

    async fn status(&self, job_id: &JobId) -> Result<JobStatus, JobError> {
        let entry = self.jobs.get(job_id)
            .ok_or_else(|| JobError::NotFound(job_id.clone()))?;
        let status = entry.status.read().await;
        Ok(status.clone())
    }
}

impl InMemoryJobBackend {
    async fn execute_job(
        agent_manager: &Arc<AgentManager>,
        job: &JobRequest,
        cancel_token: CancellationToken,
    ) -> Result<JobResult, JobError> {
        // 1. 获取 agent 模板
        let agent_record = agent_manager.get_template(&job.agent_id).await?
            .ok_or_else(|| JobError::AgentNotFound(job.agent_id.clone()))?;

        // 2. 创建运行时 agent
        let runtime_id = agent_manager.create_agent(&agent_record).await
            .map_err(|_| JobError::AgentCreationFailed)?;

        // 3. 创建 thread
        let thread_id = {
            let agent = agent_manager.get(runtime_id)
                .ok_or(JobError::AgentCreationFailed)?;
            agent.create_thread(ThreadConfig::default())
        };

        // 4. 执行（支持取消）
        let result = tokio::select! {
            _ = cancel_token.cancelled() => {
                let _ = agent_manager.delete(runtime_id);
                return Err(JobError::Cancelled);
            }
            result = Self::run_thread(agent_manager, runtime_id, thread_id, job.prompt.clone()) => {
                let _ = agent_manager.delete(runtime_id);
                result
            }
        };

        result
    }

    async fn run_thread(
        agent_manager: &Arc<AgentManager>,
        runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
        prompt: String,
    ) -> Result<JobResult, JobError> {
        let agent = agent_manager.get(runtime_id)
            .ok_or(JobError::AgentCreationFailed)?;

        let mut thread = agent.get_thread_mut(&thread_id)
            .ok_or(JobError::ThreadNotFound)?;

        let handle = thread.send_message(prompt).await;
        let output = handle.wait_for_result().await
            .map_err(|e| JobError::ExecutionFailed(e.to_string()))?;

        // 提取最终消息作为摘要
        let summary = output.messages.last()
            .map(|m| m.content.clone())
            .unwrap_or_default();

        Ok(JobResult {
            summary,
            token_usage: output.token_usage,
        })
    }
}
```

## 附录 B：PersistentJobBackend 事件订阅

```rust
pub struct PersistentJobBackend {
    job_repository: Arc<dyn JobRepository>,
    event_hub: Arc<EventHub>,  // 全局事件中心
}

#[async_trait]
impl JobBackend for PersistentJobBackend {
    async fn wait(&self, job_id: &JobId) -> Result<JobResult, JobError> {
        let mut subscriber = self.event_hub.subscribe(job_id);

        loop {
            match subscriber.recv().await {
                Ok(Event::JobStatusChanged { id, status }) if id == *job_id => {
                    match status {
                        JobStatus::Succeeded => {
                            let record = self.job_repository.find_by_id(job_id).await?
                                .ok_or(JobError::NotFound(job_id.clone()))?;
                            return Ok(JobResult {
                                summary: record.result.unwrap_or_default(),
                                token_usage: record.token_usage.unwrap_or_default(),
                            });
                        }
                        JobStatus::Failed => {
                            return Err(JobError::ExecutionFailed(
                                record.error.unwrap_or_else(|| "Unknown error".into())
                            ));
                        }
                        JobStatus::Cancelled => return Err(JobError::Cancelled),
                        JobStatus::TimedOut => return Err(JobError::Timeout),
                        _ => continue,
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(JobError::ChannelClosed);
                }
                _ => continue,
            }
        }
    }
}
```

## 附录 C：编排模式用户确认流程

```rust
impl DispatchTool {
    async fn handle_orchestrate_mode(
        &self,
        task: String,
        context: Option<String>,
        summary_hint: Option<String>,
    ) -> Result<Value, ToolError> {
        // 1. 先广播请求用户确认
        let confirmation_id = Uuid::new_v4().to_string();
        let _ = self.thread_event_sender.send(ThreadEvent::OrchestrationConfirmationRequired {
            confirmation_id: confirmation_id.clone(),
            task: task.clone(),
            message: "主 agent 请求使用编排模式，派发后将不再跟踪此任务。是否同意？".into(),
        });

        // 2. 等待用户确认（通过 ApprovalManager）
        let approval = self.approval_manager.request_approval(
            ApprovalRequest {
                id: ApprovalId::new(confirmation_id),
                action: "orchestrate_mode".into(),
                summary: format!("派发编排任务: {}", task.chars().take(50).collect::<String>()),
                risk_level: RiskLevel::Medium,
            }
        ).await.map_err(|e| ToolError::ApprovalFailed(e.to_string()))?;

        if !approval.is_approved() {
            return Ok(json!({
                "status": "cancelled",
                "message": "用户拒绝了编排模式请求"
            }));
        }

        // 3. 用户确认后，派发任务
        let job = JobRequest {
            agent_id: self.subagent_template_id.clone(),
            prompt: self.build_prompt(task.clone(), context, summary_hint),
            timeout_secs: 3600,
            backend: JobBackendKind::Persistent,
        };

        let job_id = self.job_backend.submit(job).await?;

        let _ = self.thread_event_sender.send(ThreadEvent::OrchestratedJobDispatched {
            job_id: job_id.clone(),
            task,
            message: "编排任务已派发，主 agent 不再跟踪。您可以继续对话。".into(),
        });

        Ok(json!({
            "status": "dispatched",
            "job_id": job_id.as_ref(),
            "message": "任务已派发到后台执行，完成后会通知您"
        }))
    }
}
```

## 附录 D：DispatchToolConfig 配置

```rust
#[derive(Clone, Debug)]
pub struct DispatchToolConfig {
    /// 默认超时时间（秒），用于内存模式
    pub default_timeout_secs: u64,

    /// 进度提醒间隔（秒）
    pub progress_notify_interval_secs: u64,

    /// 编排模式超时时间（秒）
    pub orchestrate_timeout_secs: u64,

    /// Subagent 模板 ID
    pub subagent_template_id: AgentId,
}

impl Default for DispatchToolConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: 300,        // 5 分钟
            progress_notify_interval_secs: 60, // 每分钟提醒
            orchestrate_timeout_secs: 3600,    // 1 小时
            subagent_template_id: AgentId::new("subagent"),
        }
    }
}
```

## 附录 E：ThreadEvent 与 thread_id 一致性

为保持与现有 ThreadEvent 的一致性，新增事件也包含 `thread_id`：

```rust
pub enum ThreadEvent {
    // === 现有事件（参考） ===
    // Processing { thread_id: ThreadId, turn_number: u32, event: LlmStreamEvent },
    // TurnCompleted { thread_id: ThreadId, turn_number: u32, token_usage: TokenUsage },

    // === Subagent 相关事件 ===
    WaitingForSubagent {
        thread_id: ThreadId,  // 主 agent 的 thread_id
        job_id: JobId,
        message: String,
    },

    SubagentProgress {
        thread_id: ThreadId,
        job_id: JobId,
        elapsed_secs: u64,
        message: String,
    },

    SubagentCompleted {
        thread_id: ThreadId,
        job_id: JobId,
        summary: String,
    },

    SubagentFailed {
        thread_id: ThreadId,
        job_id: JobId,
        error: String,
    },

    SubagentTimedOut {
        thread_id: ThreadId,
        job_id: JobId,
        timeout_secs: u64,
    },

    OrchestratedJobDispatched {
        thread_id: ThreadId,
        job_id: JobId,
        task: String,
        message: String,
    },

    OrchestrationConfirmationRequired {
        thread_id: ThreadId,
        confirmation_id: String,
        task: String,
        message: String,
    },
}
```
