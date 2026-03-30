//! get_job_result tool implementation.

use std::sync::Arc;

use argus_protocol::{
    NamedTool, RiskLevel, ThreadControlEvent, ThreadJobResult, ToolDefinition, ToolError,
    ToolExecutionContext,
};
use async_trait::async_trait;
use tokio::sync::oneshot;

use crate::job_manager::{JobLookup, JobManager};
use crate::types::{GetJobResultArgs, JobResult};

/// Tool for proactively checking whether a background job has completed.
#[derive(Debug)]
pub struct GetJobResultTool {
    job_manager: Arc<JobManager>,
}

impl GetJobResultTool {
    /// Create a new GetJobResultTool.
    pub fn new(job_manager: Arc<JobManager>) -> Self {
        Self { job_manager }
    }

    fn serialize_job_result(result: &ThreadJobResult) -> Result<serde_json::Value, ToolError> {
        serde_json::to_value(JobResult {
            success: result.success,
            message: result.message.clone(),
            token_usage: result.token_usage.clone(),
            agent_id: result.agent_id,
            agent_display_name: result.agent_display_name.clone(),
            agent_description: result.agent_description.clone(),
        })
        .map_err(|error| ToolError::ExecutionFailed {
            tool_name: "get_job_result".to_string(),
            reason: format!("failed to serialize job result: {error}"),
        })
    }

    async fn claim_queued_runtime_result(
        &self,
        ctx: &ToolExecutionContext,
        job_id: &str,
    ) -> Result<(), ToolError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if let Err(error) = ctx
            .control_tx
            .send(ThreadControlEvent::ClaimQueuedJobResult {
                job_id: job_id.to_string(),
                reply_tx,
            })
        {
            tracing::warn!(job_id, "failed to enqueue queued-job claim: {error}");
            return Ok(());
        }

        if let Err(error) = reply_rx.await {
            tracing::warn!(job_id, "queued-job claim reply dropped: {error}");
        }

        Ok(())
    }

    fn lookup_response(
        &self,
        job_id: &str,
        lookup: JobLookup,
    ) -> Result<serde_json::Value, ToolError> {
        match lookup {
            JobLookup::NotFound => Ok(serde_json::json!({
                "job_id": job_id,
                "status": "not_found",
            })),
            JobLookup::Pending => Ok(serde_json::json!({
                "job_id": job_id,
                "status": "pending",
            })),
            JobLookup::Completed(result) => Ok(serde_json::json!({
                "job_id": result.job_id,
                "status": "completed",
                "result": Self::serialize_job_result(&result)?,
            })),
            JobLookup::Consumed(result) => Ok(serde_json::json!({
                "job_id": result.job_id,
                "status": "consumed",
                "result": Self::serialize_job_result(&result)?,
            })),
        }
    }
}

#[async_trait]
impl NamedTool for GetJobResultTool {
    fn name(&self) -> &str {
        "get_job_result"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Check whether a background job has finished. Use consume=true when you are ready to use the result now and do not want it replayed as a future queued message.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "The job ID returned by dispatch_job"
                    },
                    "consume": {
                        "type": "boolean",
                        "description": "When true, consume a completed queued result so it will not be auto-injected into a later turn"
                    }
                },
                "required": ["job_id"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let args: GetJobResultArgs =
            serde_json::from_value(input).map_err(|error| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: format!("invalid input: {error}"),
            })?;

        let consume = args.consume.unwrap_or(false);
        let lookup = self
            .job_manager
            .get_job_result_status(ctx.thread_id, &args.job_id, false);

        if consume && matches!(lookup, JobLookup::Completed(_)) {
            self.claim_queued_runtime_result(&ctx, &args.job_id).await?;
            let consumed_lookup =
                self.job_manager
                    .get_job_result_status(ctx.thread_id, &args.job_id, true);
            return self.lookup_response(&args.job_id, consumed_lookup);
        }

        self.lookup_response(&args.job_id, lookup)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use argus_protocol::{AgentId, LlmProvider, ProviderId, ProviderResolver, ThreadId};
    use argus_repository::ArgusSqlite;
    use argus_repository::traits::AgentRepository;
    use argus_template::TemplateManager;
    use argus_tool::ToolManager;
    use async_trait::async_trait;
    use sqlx::SqlitePool;
    use tokio::sync::{broadcast, mpsc};

    use super::*;

    #[derive(Debug)]
    struct DummyProviderResolver;

    #[async_trait]
    impl ProviderResolver for DummyProviderResolver {
        async fn resolve(&self, _id: ProviderId) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in get_job_result tests");
        }

        async fn default_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in get_job_result tests");
        }

        async fn resolve_with_model(
            &self,
            _id: ProviderId,
            _model: &str,
        ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in get_job_result tests");
        }
    }

    fn test_job_manager() -> Arc<JobManager> {
        let pool = SqlitePool::connect_lazy("sqlite::memory:")
            .expect("lazy sqlite pool should build for tests");
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        Arc::new(JobManager::new(
            Arc::new(TemplateManager::new(
                sqlite.clone() as Arc<dyn AgentRepository>,
                sqlite.clone(),
            )),
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            Arc::new(argus_agent::CompactorManager::with_defaults()),
            std::env::temp_dir().join("argus-job-tests"),
        ))
    }

    fn completed_job(job_id: &str) -> ThreadJobResult {
        ThreadJobResult {
            job_id: job_id.to_string(),
            success: true,
            message: "finished".to_string(),
            token_usage: None,
            agent_id: AgentId::new(8),
            agent_display_name: "Worker".to_string(),
            agent_description: "Does background work".to_string(),
        }
    }

    #[tokio::test]
    async fn consume_true_claims_runtime_result_and_marks_job_consumed() {
        let job_manager = test_job_manager();
        let tool = GetJobResultTool::new(Arc::clone(&job_manager));
        let thread_id = ThreadId::new();
        let result = completed_job("job-7");

        job_manager.record_dispatched_job(thread_id, result.job_id.clone());
        job_manager.record_completed_job_result(thread_id, result.clone());

        let (pipe_tx, _) = broadcast::channel(8);
        let (control_tx, mut control_rx) = mpsc::unbounded_channel();

        let reply_result = result.clone();
        tokio::spawn(async move {
            match control_rx.recv().await {
                Some(ThreadControlEvent::ClaimQueuedJobResult { job_id, reply_tx }) => {
                    assert_eq!(job_id, "job-7");
                    let _ = reply_tx.send(Some(reply_result));
                }
                other => panic!("unexpected control event: {other:?}"),
            }
        });

        let response = tool
            .execute(
                serde_json::json!({
                    "job_id": "job-7",
                    "consume": true,
                }),
                Arc::new(ToolExecutionContext {
                    thread_id,
                    pipe_tx,
                    control_tx,
                }),
            )
            .await
            .expect("tool should succeed");

        assert_eq!(
            response.get("status"),
            Some(&serde_json::json!("completed"))
        );
        assert_eq!(
            response.pointer("/result/message"),
            Some(&serde_json::json!("finished"))
        );
        assert!(matches!(
            job_manager.get_job_result_status(thread_id, "job-7", false),
            JobLookup::Consumed(found) if found.job_id == "job-7"
        ));
    }
}
