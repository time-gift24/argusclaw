use super::*;

use chrono::Utc;
use tokio::sync::broadcast;
use uuid::Uuid;

pub(super) struct JobExecutionContext<'a> {
    pub(super) request: &'a JobExecutionRequest,
    pub(super) execution_thread_id: ThreadId,
    pub(super) pipe_tx: &'a broadcast::Sender<ThreadEvent>,
}

impl JobManager {
    pub(super) fn emit_dispatched_job_events(
        &self,
        job_id: &str,
        execution_thread_id: ThreadId,
        pipe_tx: &broadcast::Sender<ThreadEvent>,
    ) {
        let _ = pipe_tx.send(ThreadEvent::ThreadBoundToJob {
            job_id: job_id.to_string(),
            thread_id: execution_thread_id,
        });
        if let Some(runtime) = self.job_runtime_summary(&execution_thread_id) {
            Self::emit_job_runtime_updated(pipe_tx, &runtime);
        }
        let _ = pipe_tx.send(ThreadEvent::JobRuntimeQueued {
            thread_id: execution_thread_id,
            job_id: job_id.to_string(),
        });
        self.emit_job_runtime_metrics(pipe_tx);
    }

    pub(super) async fn subscribe_or_complete_with_failure(
        &self,
        request: &JobExecutionRequest,
        execution_thread_id: ThreadId,
        fallback_agent_id: AgentId,
        fallback_display_name: &str,
    ) -> Result<broadcast::Receiver<ThreadEvent>, ThreadJobResult> {
        match self.thread_pool.subscribe(&execution_thread_id) {
            Some(rx) => Ok(rx),
            None => {
                let result = Self::failure_result(
                    request.job_id.clone(),
                    fallback_agent_id,
                    fallback_display_name.to_string(),
                    String::new(),
                    format!(
                        "job runtime {} is missing a runtime event stream",
                        execution_thread_id
                    ),
                );
                self.persist_job_completion(&request.job_id, &result, None)
                    .await;
                Err(result)
            }
        }
    }

    pub(super) async fn mark_job_runtime_running(
        &self,
        context: &JobExecutionContext<'_>,
        thread: &ThreadHandle,
    ) -> String {
        let started_at = Utc::now().to_rfc3339();
        let estimated_memory_bytes =
            ThreadPool::estimate_thread_memory(thread) + context.request.prompt.len() as u64;
        self.thread_pool.mark_runtime_running(
            &context.execution_thread_id,
            estimated_memory_bytes,
            started_at.clone(),
        );
        if let Err(error) = self
            .persist_job_status(
                &context.request.job_id,
                JobStatus::Running,
                Some(started_at.as_str()),
                None,
            )
            .await
        {
            tracing::warn!(
                job_id = %context.request.job_id,
                error = %error,
                "Failed to persist running job status"
            );
        }
        let runtime = self.upsert_job_runtime_summary(
            context.execution_thread_id,
            context.request.job_id.clone(),
            ThreadRuntimeStatus::Running,
            estimated_memory_bytes,
            Some(started_at.clone()),
            true,
            None,
        );
        Self::emit_job_runtime_updated(context.pipe_tx, &runtime);
        let _ = context.pipe_tx.send(ThreadEvent::JobRuntimeStarted {
            thread_id: context.execution_thread_id,
            job_id: context.request.job_id.clone(),
        });
        self.emit_job_runtime_metrics(context.pipe_tx);
        started_at
    }

    pub(super) fn build_task_assignment(
        &self,
        request: &JobExecutionRequest,
        execution_thread_id: ThreadId,
        started_at: &str,
        from_label: String,
    ) -> MailboxMessage {
        MailboxMessage {
            id: Uuid::new_v4().to_string(),
            from_thread_id: request.originating_thread_id,
            to_thread_id: execution_thread_id,
            from_label,
            message_type: MailboxMessageType::TaskAssignment {
                task_id: request.job_id.clone(),
                subject: Self::task_subject(&request.prompt),
                description: request.prompt.clone(),
            },
            text: request.prompt.clone(),
            timestamp: started_at.to_string(),
            read: false,
            summary: request.context.as_ref().map(|context| context.to_string()),
        }
    }

    pub(super) fn maybe_transition_job_runtime_to_cooling(
        &self,
        context: &JobExecutionContext<'_>,
        result: &ThreadJobResult,
        thread: &ThreadHandle,
    ) {
        let cooling_memory = ThreadPool::estimate_thread_memory(thread);
        let terminal_reason = if result.cancelled {
            Some(ThreadPoolEventReason::Cancelled)
        } else if result.success {
            None
        } else {
            Some(ThreadPoolEventReason::ExecutionFailed)
        };

        if self
            .thread_pool
            .transition_runtime_to_cooling(&context.execution_thread_id, Some(cooling_memory))
            .is_some()
        {
            let runtime = self.upsert_job_runtime_summary(
                context.execution_thread_id,
                context.request.job_id.clone(),
                ThreadRuntimeStatus::Cooling,
                cooling_memory,
                Some(Utc::now().to_rfc3339()),
                true,
                terminal_reason,
            );
            Self::emit_job_runtime_updated(context.pipe_tx, &runtime);
            let _ = context.pipe_tx.send(ThreadEvent::JobRuntimeCooling {
                thread_id: context.execution_thread_id,
                job_id: context.request.job_id.clone(),
            });
            self.emit_job_runtime_metrics(context.pipe_tx);
        }
    }
}
