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
    thread_event_sender: broadcast::Sender<ThreadEvent>,
    subagent_template_id: AgentId,
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

        if orchestrate {
            return self.handle_orchestrate_mode(task, context, summary_hint).await;
        }

        self.dispatch_and_wait(task, context, summary_hint).await
    }

    async fn dispatch_and_wait(
        &self,
        task: String,
        context: Option<String>,
        summary_hint: Option<String>,
    ) -> Result<Value, ToolError> {
        let job = JobRequest {
            agent_id: self.subagent_template_id(),
            prompt: self.build_prompt(task, context, summary_hint),
            timeout_secs: 300,
            backend: JobBackendKind::InMemory,
        };

        let job_id = self.job_backend.submit(job).await?;

        let _ = self.thread_event_sender.send(ThreadEvent::WaitingForSubagent {
            job_id: job_id.clone(),
            message: "等待 subagent 完成...".into(),
        });

        let notifier = self.spawn_progress_notifier(job_id.clone());

        let result = tokio::time::timeout(
            Duration::from_secs(300),
            self.job_backend.wait(&job_id),
        ).await;

        notifier.abort();

        match result {
            Ok(Ok(job_result)) => {
                let _ = self.thread_event_sender.send(ThreadEvent::SubagentCompleted {
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
                    job_id,
                    timeout_secs: 300,
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

```rust
pub enum ThreadEvent {
    // 现有事件...

    /// 开始等待 subagent
    WaitingForSubagent {
        job_id: JobId,
        message: String,
    },

    /// Subagent 进度提醒（每 N 秒）
    SubagentProgress {
        job_id: JobId,
        elapsed_secs: u64,
        message: String,
    },

    /// Subagent 完成
    SubagentCompleted {
        job_id: JobId,
        summary: String,
    },

    /// Subagent 失败
    SubagentFailed {
        job_id: JobId,
        error: String,
    },

    /// Subagent 超时
    SubagentTimedOut {
        job_id: JobId,
        timeout_secs: u64,
    },

    /// 编排任务已派发
    OrchestratedJobDispatched {
        job_id: JobId,
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
