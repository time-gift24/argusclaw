use super::support::JobExecutionContext;
use super::*;

impl JobManager {
    /// Dispatch a background job through the thread pool.
    #[allow(clippy::too_many_arguments)]
    pub async fn dispatch_job(
        &self,
        originating_thread_id: ThreadId,
        job_id: String,
        agent_id: AgentId,
        prompt: String,
        context: Option<serde_json::Value>,
        pipe_tx: broadcast::Sender<ThreadEvent>,
    ) -> Result<(), JobError> {
        if prompt.trim().is_empty() {
            return Err(JobError::ExecutionFailed(
                "prompt cannot be empty".to_string(),
            ));
        }

        let request = JobExecutionRequest {
            originating_thread_id,
            job_id: job_id.clone(),
            agent_id,
            prompt,
            context,
        };

        let execution_thread_id = self.enqueue_job_runtime(&request).await?;
        self.notify_job_thread_created(originating_thread_id, execution_thread_id);

        let cancellation = TurnCancellation::new();
        let spawn_cancellation = cancellation.clone();
        let manager = self.clone();
        Self::record_dispatched_job_in_store(
            &self.tracked_jobs,
            originating_thread_id,
            job_id.clone(),
            cancellation,
        );
        self.emit_dispatched_job_events(job_id.as_str(), execution_thread_id, &pipe_tx);

        let pipe_tx_clone = pipe_tx.clone();

        tokio::spawn(async move {
            let result = manager
                .execute_job_runtime(
                    request,
                    execution_thread_id,
                    pipe_tx_clone.clone(),
                    spawn_cancellation,
                )
                .await;

            manager
                .forward_job_result_to_runtime(
                    originating_thread_id,
                    execution_thread_id,
                    result.clone(),
                )
                .await;
            Self::record_completed_job_result_in_store(
                &manager.tracked_jobs,
                originating_thread_id,
                result.clone(),
            );
            Self::broadcast_job_result(&pipe_tx_clone, originating_thread_id, result);
        });

        Ok(())
    }

    async fn enqueue_job_runtime(
        &self,
        request: &JobExecutionRequest,
    ) -> Result<ThreadId, JobError> {
        let now = Utc::now().to_rfc3339();
        let thread_id = self.persist_binding(request, &now).await?;
        self.persist_job_status(&request.job_id, JobStatus::Queued, None, None)
            .await?;
        self.thread_pool.register_runtime(
            thread_id,
            ThreadRuntimeStatus::Queued,
            request.prompt.len() as u64,
            Some(now.clone()),
            true,
            None,
            None,
        );
        self.upsert_job_runtime_summary(
            thread_id,
            request.job_id.clone(),
            ThreadRuntimeStatus::Queued,
            request.prompt.len() as u64,
            Some(now),
            true,
            None,
        );
        self.sync_job_runtime_metadata(
            thread_id,
            Some(request.job_id.clone()),
            Some(request.originating_thread_id),
        );
        Ok(thread_id)
    }

    async fn execute_job_runtime(
        &self,
        request: JobExecutionRequest,
        execution_thread_id: ThreadId,
        pipe_tx: broadcast::Sender<ThreadEvent>,
        cancellation: TurnCancellation,
    ) -> ThreadJobResult {
        let fallback_job_id = request.job_id.clone();
        let fallback_agent_id = request.agent_id;
        let fallback_display_name = format!("Agent {}", fallback_agent_id.inner());
        let thread = match self
            .ensure_job_runtime(&request, execution_thread_id, &pipe_tx)
            .await
        {
            Ok(thread) => thread,
            Err(error) => {
                let result = Self::failure_result(
                    fallback_job_id,
                    fallback_agent_id,
                    fallback_display_name,
                    String::new(),
                    error.to_string(),
                );
                self.persist_job_completion(&request.job_id, &result, None)
                    .await;
                return result;
            }
        };
        let runtime_rx = match self
            .subscribe_or_complete_with_failure(
                &request,
                execution_thread_id,
                fallback_agent_id,
                &fallback_display_name,
            )
            .await
        {
            Ok(rx) => rx,
            Err(result) => return result,
        };
        let execution_context = JobExecutionContext {
            request: &request,
            execution_thread_id,
            pipe_tx: &pipe_tx,
        };
        let started_at = self
            .mark_job_runtime_running(&execution_context, &thread)
            .await;

        let cancellation_for_wait = cancellation.clone();

        let result = if request.prompt == "__panic_thread_pool_execute_turn__" {
            Self::failure_result(
                fallback_job_id.clone(),
                fallback_agent_id,
                fallback_display_name,
                String::new(),
                "job executor panicked: thread pool panic test hook".to_string(),
            )
        } else {
            let task_assignment = self.build_task_assignment(
                &request,
                execution_thread_id,
                &started_at,
                self.thread_display_label(&request.originating_thread_id)
                    .await,
            );

            if cancellation_for_wait.is_cancelled() {
                Self::cancelled_result(
                    fallback_job_id,
                    fallback_agent_id,
                    fallback_display_name,
                    String::new(),
                    "Turn cancelled".to_string(),
                )
            } else {
                match self
                    .thread_pool
                    .deliver_thread_message(
                        execution_thread_id,
                        Self::route_mailbox_message(task_assignment),
                    )
                    .await
                {
                    Ok(()) => {
                        let cancellation_thread = thread.clone();
                        let cancellation_signal = cancellation.clone();
                        let cancellation_forwarder = tokio::spawn(async move {
                            cancellation_signal.cancelled().await;
                            let _ = cancellation_thread.send_message(ThreadMessage::Interrupt);
                        });

                        let result = self
                            .await_job_turn_result(
                                execution_thread_id,
                                &thread,
                                runtime_rx,
                                request.job_id.clone(),
                                cancellation_for_wait,
                            )
                            .await;
                        cancellation_forwarder.abort();
                        result
                    }
                    Err(error) => Self::failure_result(
                        fallback_job_id,
                        fallback_agent_id,
                        fallback_display_name,
                        String::new(),
                        error.to_string(),
                    ),
                }
            }
        };
        self.persist_thread_stats(&execution_thread_id, &thread)
            .await;
        self.persist_job_completion(&request.job_id, &result, Some(started_at.as_str()))
            .await;
        self.maybe_transition_job_runtime_to_cooling(&execution_context, &result, &thread);

        result
    }

    async fn ensure_job_runtime(
        &self,
        request: &JobExecutionRequest,
        thread_id: ThreadId,
        pipe_tx: &broadcast::Sender<ThreadEvent>,
    ) -> Result<ThreadHandle, JobError> {
        let runtime = self.upsert_job_runtime_summary(
            thread_id,
            request.job_id.clone(),
            ThreadRuntimeStatus::Loading,
            0,
            Some(Utc::now().to_rfc3339()),
            true,
            None,
        );
        Self::emit_job_runtime_updated(pipe_tx, &runtime);
        self.emit_job_runtime_metrics(pipe_tx);

        let manager = self.clone();
        let request_for_build = request.clone();
        let job_id = request.job_id.clone();
        let thread = if let Some(thread) = self.thread_pool.loaded_runtime(&thread_id) {
            thread
        } else {
            match self
                .thread_pool
                .load_runtime_with_builder(thread_id, "job thread", false, None, true, move || {
                    let manager = manager.clone();
                    let request = request_for_build.clone();
                    async move {
                        manager
                            .build_job_thread(&request, thread_id)
                            .await
                            .map_err(|error| ThreadPoolError::ExecutionFailed(error.to_string()))
                    }
                })
                .await
            {
                Ok(thread) => thread,
                Err(error) => {
                    let error = Self::map_pool_error(error);
                    let runtime = self.upsert_job_runtime_summary(
                        thread_id,
                        job_id,
                        ThreadRuntimeStatus::Inactive,
                        0,
                        Some(Utc::now().to_rfc3339()),
                        true,
                        Some(ThreadPoolEventReason::ExecutionFailed),
                    );
                    Self::emit_job_runtime_updated(pipe_tx, &runtime);
                    self.emit_job_runtime_metrics(pipe_tx);
                    return Err(error);
                }
            }
        };
        Ok(thread)
    }

    async fn build_job_thread(
        &self,
        request: &JobExecutionRequest,
        thread_id: ThreadId,
    ) -> Result<Thread, JobError> {
        let thread_record = if let Some(thread_repository) = self.thread_repository() {
            thread_repository
                .get_thread(&thread_id)
                .await
                .map_err(|err| {
                    JobError::ExecutionFailed(format!("failed to load thread record: {err}"))
                })?
        } else {
            None
        };
        let base_dir = find_job_thread_base_dir(&self.trace_dir, thread_id)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let metadata = recover_and_validate_metadata(&base_dir, thread_id, ThreadTraceKind::Job)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        if metadata.parent_thread_id != Some(request.originating_thread_id) {
            return Err(JobError::ExecutionFailed(format!(
                "job thread {} is bound to parent {:?}, not {}",
                thread_id, metadata.parent_thread_id, request.originating_thread_id
            )));
        }
        if metadata.job_id.as_deref() != Some(request.job_id.as_str()) {
            return Err(JobError::ExecutionFailed(format!(
                "job thread {} is bound to job {:?}, not {}",
                thread_id, metadata.job_id, request.job_id
            )));
        }
        let agent_record = metadata.agent_snapshot.clone();
        let provider = if let Some(thread_record) = thread_record.as_ref() {
            let provider_id = ProviderId::new(thread_record.provider_id.into_inner());
            self.resolve_provider_with_fallback(
                provider_id,
                thread_record.model_override.as_deref(),
            )
            .await
        } else if let Some(provider_id) = agent_record.provider_id {
            self.resolve_provider_with_fallback(provider_id, agent_record.model_id.as_deref())
                .await
        } else {
            self.provider_resolver.default_provider().await
        }
        .map_err(|err| JobError::ExecutionFailed(format!("failed to resolve provider: {err}")))?;

        let config = build_thread_config(base_dir.clone(), provider.model_name().to_string())
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let plan_store = FilePlanStore::new(base_dir.clone());
        let thread_title = thread_record
            .as_ref()
            .and_then(|record| record.title.clone())
            .or_else(|| Some(format!("job:{}", request.job_id)));
        let mut builder = ThreadBuilder::new()
            .id(thread_id)
            .session_id(Self::job_runtime_session_id(thread_id))
            .agent_record(Arc::new(agent_record))
            .title(thread_title)
            .provider(provider.clone())
            .tool_manager(Arc::clone(&self.tool_manager))
            .compactor(Arc::new(LlmThreadCompactor::new(provider)))
            .plan_store(plan_store)
            .config(config);
        if let Some(resolver) = self.current_mcp_tool_resolver() {
            builder = builder.mcp_tool_resolver(resolver);
        }
        let mut thread = builder
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        self.sync_job_runtime_metadata(
            metadata.thread_id,
            metadata.job_id.clone(),
            metadata.parent_thread_id,
        );

        if let Some(thread_record) = thread_record {
            hydrate_turn_log_state(&mut thread, &base_dir, &thread_record.updated_at)
                .await
                .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        }

        Ok(thread)
    }

    fn job_runtime_session_id(thread_id: ThreadId) -> SessionId {
        SessionId(*thread_id.inner())
    }

    async fn summarize_thread_history(thread: &ThreadHandle) -> String {
        const SUMMARY_LIMIT: usize = 4000;

        let summary = thread
            .history()
            .into_iter()
            .rev()
            .find_map(|message| match message {
                ChatMessage {
                    role: Role::Assistant,
                    content,
                    ..
                } if !content.is_empty() => Some(content),
                _ => None,
            });

        match summary {
            Some(content) => {
                let mut chars = content.chars();
                let summary: String = chars.by_ref().take(SUMMARY_LIMIT).collect();
                if chars.next().is_some() {
                    format!("{summary}...")
                } else {
                    content
                }
            }
            None => "job completed".to_string(),
        }
    }

    async fn await_job_turn_result(
        &self,
        execution_thread_id: ThreadId,
        thread: &ThreadHandle,
        mut runtime_rx: broadcast::Receiver<ThreadEvent>,
        fallback_job_id: String,
        cancellation: TurnCancellation,
    ) -> ThreadJobResult {
        let agent_id = thread.agent_id();
        let agent_display_name = thread.agent_display_name();
        let agent_description = thread.agent_description();

        let mut token_usage = None;
        let mut failure = None;
        let thread_id_str = execution_thread_id.inner().to_string();
        let mut terminal_turn_number = None;

        loop {
            match runtime_rx.recv().await {
                Ok(ThreadEvent::TurnCompleted {
                    thread_id,
                    turn_number,
                    token_usage: completed_usage,
                    ..
                }) if thread_id == thread_id_str => {
                    token_usage = Some(completed_usage);
                    terminal_turn_number = Some(turn_number);
                }
                Ok(ThreadEvent::TurnFailed {
                    thread_id,
                    turn_number,
                    error,
                }) if thread_id == thread_id_str => {
                    failure = Some(error);
                    terminal_turn_number = Some(turn_number);
                }
                Ok(ThreadEvent::TurnSettled {
                    thread_id,
                    turn_number,
                }) if thread_id == thread_id_str => {
                    if terminal_turn_number == Some(turn_number) {
                        break;
                    }
                }
                Ok(ThreadEvent::Idle { thread_id }) if thread_id == thread_id_str => {
                    if terminal_turn_number.is_some() {
                        continue;
                    }

                    let message = if cancellation.is_cancelled() {
                        "Turn cancelled".to_string()
                    } else {
                        "job runtime became idle without a terminal turn result".to_string()
                    };
                    return if cancellation.is_cancelled() {
                        Self::cancelled_result(
                            fallback_job_id,
                            agent_id,
                            agent_display_name,
                            agent_description,
                            message,
                        )
                    } else {
                        Self::failure_result(
                            fallback_job_id,
                            agent_id,
                            agent_display_name,
                            agent_description,
                            message,
                        )
                    };
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Self::failure_result(
                        fallback_job_id,
                        agent_id,
                        agent_display_name,
                        agent_description,
                        "job runtime event stream closed unexpectedly".to_string(),
                    );
                }
            }
        }

        if let Some(message) = failure {
            return if cancellation.is_cancelled() {
                Self::cancelled_result(
                    fallback_job_id,
                    agent_id,
                    agent_display_name,
                    agent_description,
                    message,
                )
            } else {
                Self::failure_result(
                    fallback_job_id,
                    agent_id,
                    agent_display_name,
                    agent_description,
                    message,
                )
            };
        }

        ThreadJobResult {
            job_id: fallback_job_id,
            success: true,
            cancelled: false,
            message: Self::summarize_thread_history(thread).await,
            token_usage,
            agent_id,
            agent_display_name,
            agent_description,
        }
    }

    pub(super) fn failure_result(
        job_id: String,
        agent_id: AgentId,
        agent_display_name: String,
        agent_description: String,
        message: String,
    ) -> ThreadJobResult {
        ThreadJobResult {
            job_id,
            success: false,
            cancelled: false,
            message,
            token_usage: None,
            agent_id,
            agent_display_name,
            agent_description,
        }
    }

    fn cancelled_result(
        job_id: String,
        agent_id: AgentId,
        agent_display_name: String,
        agent_description: String,
        message: String,
    ) -> ThreadJobResult {
        ThreadJobResult {
            job_id,
            success: false,
            cancelled: true,
            message,
            token_usage: None,
            agent_id,
            agent_display_name,
            agent_description,
        }
    }

    fn map_pool_error(error: ThreadPoolError) -> JobError {
        JobError::ExecutionFailed(error.to_string())
    }

    async fn resolve_provider_with_fallback(
        &self,
        provider_id: ProviderId,
        model: Option<&str>,
    ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
        match model {
            Some(model) => match self
                .provider_resolver
                .resolve_with_model(provider_id, model)
                .await
            {
                Ok(provider) => Ok(provider),
                Err(_) => self.provider_resolver.resolve(provider_id).await,
            },
            None => self.provider_resolver.resolve(provider_id).await,
        }
    }
}
