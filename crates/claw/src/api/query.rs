// crates/claw/src/api/query.rs
use super::types::{ContextUsage, Job, Workflow};
use crate::db::thread::ThreadRepository;
use crate::db::llm::LlmProviderRepository;
use crate::job::JobRepository;
use crate::protocol::ThreadId;
use crate::workflow::{WorkflowId, WorkflowRepository};
use async_graphql::{Context, ID, Object};

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn workflow(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<Option<Workflow>> {
        let workflow_repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let job_repo = ctx.data::<Box<dyn JobRepository>>()?;
        let workflow_id = WorkflowId::new(id.to_string());

        let record = workflow_repo.get_workflow(&workflow_id).await?;
        let Some(wf) = record else { return Ok(None) };

        let jobs = job_repo.list_by_group(id.as_ref()).await?;

        Ok(Some(Workflow {
            id: wf.id.to_string(),
            name: wf.name,
            status: wf.status.to_string(),
            jobs: jobs
                .into_iter()
                .map(|j| Job {
                    id: j.id.to_string(),
                    job_type: j.job_type.to_string(),
                    name: j.name,
                    status: j.status.to_string(),
                    agent_id: j.agent_id.to_string(),
                    context: j.context,
                    prompt: j.prompt,
                    thread_id: j.thread_id.map(|t| t.to_string()),
                    group_id: j.group_id,
                    depends_on: j.depends_on.iter().map(|j| j.to_string()).collect(),
                    cron_expr: j.cron_expr,
                    started_at: j.started_at,
                    finished_at: j.finished_at,
                })
                .collect(),
            created_at: String::new(),
            updated_at: String::new(),
        }))
    }

    async fn workflows(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Workflow>> {
        let workflow_repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let job_repo = ctx.data::<Box<dyn JobRepository>>()?;
        let records = workflow_repo.list_workflows().await?;

        let mut workflows = Vec::new();
        for wf in records {
            let jobs = job_repo.list_by_group(wf.id.as_ref()).await?;

            workflows.push(Workflow {
                id: wf.id.to_string(),
                name: wf.name,
                status: wf.status.to_string(),
                jobs: jobs
                    .into_iter()
                    .map(|j| Job {
                        id: j.id.to_string(),
                        job_type: j.job_type.to_string(),
                        name: j.name,
                        status: j.status.to_string(),
                        agent_id: j.agent_id.to_string(),
                        context: j.context,
                        prompt: j.prompt,
                        thread_id: j.thread_id.map(|t| t.to_string()),
                        group_id: j.group_id,
                        depends_on: j.depends_on.iter().map(|j| j.to_string()).collect(),
                        cron_expr: j.cron_expr,
                        started_at: j.started_at,
                        finished_at: j.finished_at,
                    })
                    .collect(),
                created_at: String::new(),
                updated_at: String::new(),
            });
        }

        Ok(workflows)
    }

    async fn thread_context_usage(&self, ctx: &Context<'_>, thread_id: ID) -> async_graphql::Result<Option<ContextUsage>> {
        let thread_repo = ctx.data::<Box<dyn ThreadRepository>>()?;
        let provider_repo = ctx.data::<Box<dyn LlmProviderRepository>>()?;

        let thread_id = ThreadId::parse(thread_id.as_ref()).map_err(|e| async_graphql::Error::new(e.to_string()))?;
        let thread_record = thread_repo.get_thread(&thread_id).await?;

        let Some(thread) = thread_record else { return Ok(None) };

        // Get the provider to find the model's context_window
        let provider = provider_repo.get_provider(&thread.provider_id).await?;
        let Some(provider) = provider else { return Ok(None) };

        // Find the default model's context_window
        let model_context_window = provider
            .models
            .iter()
            .find(|m| m.id == provider.default_model)
            .map(|m| m.context_window)
            .unwrap_or(128_000);

        let usage_ratio = if model_context_window > 0 {
            thread.token_count as f64 / model_context_window as f64
        } else {
            0.0
        };

        // For now, we use the stored token_count as total_tokens
        // In a full implementation, we'd track input/output/cached tokens separately
        let total_tokens = thread.token_count as i32;

        Ok(Some(ContextUsage {
            model_context_window: model_context_window as i32,
            input_tokens: total_tokens / 2,
            cached_tokens: 0,
            output_tokens: total_tokens / 2,
            total_tokens,
            usage_ratio,
        }))
    }
}
