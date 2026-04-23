use super::*;

impl JobManager {
    pub(super) async fn persist_thread_stats(&self, thread_id: &ThreadId, thread: &ThreadHandle) {
        let Some(thread_repository) = self.thread_repository() else {
            return;
        };
        let token_count = thread.token_count();
        let turn_count = thread.turn_count();
        if let Err(error) = thread_repository
            .update_thread_stats(thread_id, token_count, turn_count)
            .await
        {
            tracing::warn!(
                thread_id = %thread_id,
                error = %error,
                "Failed to persist job thread stats"
            );
        }
    }

    pub(super) async fn persist_job_status(
        &self,
        job_id: &str,
        status: JobStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), JobError> {
        let Some(job_repository) = self.job_repository.as_ref() else {
            return Ok(());
        };
        job_repository
            .update_status(&JobId::new(job_id), status, started_at, finished_at)
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to persist job status: {err}"))
            })
    }

    pub(super) async fn persist_job_completion(
        &self,
        job_id: &str,
        result: &ThreadJobResult,
        started_at: Option<&str>,
    ) {
        let Some(job_repository) = self.job_repository.as_ref() else {
            return;
        };
        let persisted_result = JobResult {
            success: result.success,
            message: result.message.clone(),
            token_usage: result.token_usage.clone(),
            agent_id: RepoAgentId::new(result.agent_id.inner()),
            agent_display_name: result.agent_display_name.clone(),
            agent_description: result.agent_description.clone(),
        };
        if let Err(error) = job_repository
            .update_result(&JobId::new(job_id), &persisted_result)
            .await
        {
            tracing::warn!(
                job_id,
                error = %error,
                "Failed to persist job result"
            );
            return;
        }

        let finished_at = Utc::now().to_rfc3339();
        let status = if result.success {
            JobStatus::Succeeded
        } else if result.cancelled {
            JobStatus::Cancelled
        } else {
            JobStatus::Failed
        };
        if let Err(error) = job_repository
            .update_status(
                &JobId::new(job_id),
                status,
                started_at,
                Some(finished_at.as_str()),
            )
            .await
        {
            tracing::warn!(
                job_id,
                error = %error,
                "Failed to persist final job status"
            );
        }
    }

    pub(super) async fn persist_binding(
        &self,
        request: &JobExecutionRequest,
        now: &str,
    ) -> Result<ThreadId, JobError> {
        if self.thread_repository.is_none()
            || self.provider_repository.is_none()
            || self.job_repository.is_none()
        {
            return Ok(ThreadId::new());
        }

        let agent_record = self
            .template_manager
            .get(request.agent_id)
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!(
                    "failed to load agent {}: {err}",
                    request.agent_id.inner()
                ))
            })?;
        let agent_record = agent_record.ok_or_else(|| {
            JobError::ExecutionFailed(format!("agent {} not found", request.agent_id.inner()))
        })?;
        let parent_base_dir = self
            .trace_base_dir_for_thread(request.originating_thread_id)
            .await?;
        let parent_metadata = recover_thread_metadata(&parent_base_dir)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;

        let Some(thread_repository) = self.thread_repository() else {
            return Ok(ThreadId::new());
        };
        let Some(provider_repository) = self.provider_repository() else {
            return Ok(ThreadId::new());
        };
        let Some(job_repository) = self.job_repository.as_ref() else {
            return Ok(ThreadId::new());
        };

        let job_id = JobId::new(request.job_id.clone());
        let existing_job = job_repository.get(&job_id).await.map_err(|err| {
            JobError::ExecutionFailed(format!("failed to load job record: {err}"))
        })?;
        let existing_thread_id = existing_job.as_ref().and_then(|job| job.thread_id);
        let existing_thread = if let Some(thread_id) = existing_thread_id {
            thread_repository
                .get_thread(&thread_id)
                .await
                .map_err(|err| {
                    JobError::ExecutionFailed(format!("failed to load thread record: {err}"))
                })?
        } else {
            None
        };

        let thread_id = existing_thread_id.unwrap_or_else(ThreadId::new);
        let should_cleanup_trace_dir = existing_thread_id.is_none();
        let default_base_dir = child_thread_base_dir(&parent_base_dir, thread_id);
        let base_dir = if existing_thread_id.is_some() {
            let existing_base_dir = find_job_thread_base_dir(&self.trace_dir, thread_id)
                .await
                .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
            if existing_base_dir != default_base_dir {
                return Err(JobError::ExecutionFailed(format!(
                    "job thread {} cannot move between parents without trace migration",
                    thread_id
                )));
            }
            let metadata = recover_thread_metadata(&existing_base_dir)
                .await
                .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
            if metadata.parent_thread_id != Some(request.originating_thread_id) {
                return Err(JobError::ExecutionFailed(format!(
                    "job thread {} is already bound to parent {:?}",
                    thread_id, metadata.parent_thread_id
                )));
            }
            if metadata.job_id.as_deref() != Some(request.job_id.as_str()) {
                return Err(JobError::ExecutionFailed(format!(
                    "job thread {} is already bound to job {:?}",
                    thread_id, metadata.job_id
                )));
            }
            existing_base_dir
        } else {
            default_base_dir
        };

        let metadata = ThreadTraceMetadata {
            thread_id,
            kind: ThreadTraceKind::Job,
            root_session_id: parent_metadata.root_session_id,
            parent_thread_id: Some(request.originating_thread_id),
            job_id: Some(request.job_id.clone()),
            agent_snapshot: agent_record.clone(),
        };
        persist_thread_metadata(&base_dir, &metadata)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        self.sync_job_runtime_metadata(
            metadata.thread_id,
            metadata.job_id.clone(),
            metadata.parent_thread_id,
        );

        let template_provider_id = agent_record
            .provider_id
            .map(|id| argus_protocol::LlmProviderId::new(id.inner()));
        let provider_id = match template_provider_id {
            Some(provider_id) => provider_id,
            None => provider_repository
                .get_default_provider_id()
                .await
                .map_err(|err| {
                    JobError::ExecutionFailed(format!(
                        "failed to resolve default provider id: {err}"
                    ))
                })?
                .ok_or_else(|| {
                    JobError::ExecutionFailed("default provider is not configured".to_string())
                })?,
        };
        let model_override = agent_record.model_id.clone();

        let mut thread_record = existing_thread.unwrap_or(ThreadRecord {
            id: thread_id,
            provider_id,
            title: Some(format!("job:{}", request.job_id)),
            token_count: 0,
            turn_count: 0,
            session_id: None,
            template_id: Some(RepoAgentId::new(request.agent_id.inner())),
            model_override: model_override.clone(),
            created_at: now.to_string(),
            updated_at: now.to_string(),
        });
        thread_record.id = thread_id;
        thread_record.provider_id = provider_id;
        thread_record.title = Some(format!("job:{}", request.job_id));
        thread_record.session_id = None;
        thread_record.template_id = Some(RepoAgentId::new(request.agent_id.inner()));
        thread_record.model_override = model_override;
        thread_record.updated_at = now.to_string();
        if let Err(err) = thread_repository.upsert_thread(&thread_record).await {
            if should_cleanup_trace_dir {
                cleanup_trace_dir(&base_dir).await;
            }
            return Err(JobError::ExecutionFailed(format!(
                "failed to persist thread record: {err}"
            )));
        }

        if existing_job.is_some() {
            if existing_thread_id.is_none()
                && let Err(err) =
                    Self::persist_existing_job_binding(job_repository, &job_id, thread_id).await
            {
                if should_cleanup_trace_dir {
                    cleanup_trace_dir(&base_dir).await;
                }
                return Err(Self::rollback_thread_record(
                    &thread_repository,
                    thread_id,
                    format!("{err}"),
                )
                .await);
            }
            return Ok(thread_id);
        }

        let job_record = JobRecord {
            id: job_id,
            job_type: JobType::Standalone,
            name: format!("job:{}", request.job_id),
            status: JobStatus::Pending,
            agent_id: RepoAgentId::new(request.agent_id.inner()),
            context: request
                .context
                .as_ref()
                .map(std::string::ToString::to_string),
            prompt: request.prompt.clone(),
            thread_id: Some(thread_id),
            group_id: None,
            depends_on: Vec::new(),
            cron_expr: None,
            scheduled_at: None,
            started_at: None,
            finished_at: None,
            parent_job_id: None,
            result: None,
        };

        if let Err(err) = job_repository.create(&job_record).await {
            if should_cleanup_trace_dir {
                cleanup_trace_dir(&base_dir).await;
            }
            return Err(Self::rollback_thread_record(
                &thread_repository,
                thread_id,
                format!("failed to create job record: {err}"),
            )
            .await);
        }

        Ok(thread_id)
    }

    async fn persist_existing_job_binding(
        job_repository: &Arc<dyn JobRepository>,
        job_id: &JobId,
        thread_id: ThreadId,
    ) -> Result<(), JobError> {
        job_repository
            .update_thread_id(job_id, &thread_id)
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to persist job-thread binding: {err}"))
            })
    }

    async fn rollback_thread_record(
        thread_repository: &Arc<dyn ThreadRepository>,
        thread_id: ThreadId,
        message: String,
    ) -> JobError {
        match thread_repository.delete_thread(&thread_id).await {
            Ok(_) => JobError::ExecutionFailed(message),
            Err(cleanup_err) => JobError::ExecutionFailed(format!(
                "{message}; failed to roll back thread record: {cleanup_err}"
            )),
        }
    }
}
