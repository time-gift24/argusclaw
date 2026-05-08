use super::*;

impl JobManager {
    pub fn set_chat_mailbox_forwarder<F, Fut>(&self, forwarder: F)
    where
        F: Fn(ThreadId, MailboxMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        let forwarder = Arc::new(
            move |thread_id: ThreadId, message: MailboxMessage| -> ChatMailboxForwarderFuture {
                Box::pin(forwarder(thread_id, message))
            },
        ) as Arc<ChatMailboxForwarder>;
        let mut slot = self
            .chat_mailbox_forwarder
            .lock()
            .expect("chat mailbox forwarder mutex poisoned");
        *slot = Some(forwarder);
    }

    pub(super) async fn thread_display_label(&self, thread_id: &ThreadId) -> String {
        let Some(thread) = self.thread_pool.loaded_thread(thread_id) else {
            return format!("Thread {}", thread_id);
        };

        thread.agent_display_name()
    }

    pub(super) fn route_mailbox_message(message: MailboxMessage) -> ThreadMessage {
        if matches!(message.message_type, MailboxMessageType::JobResult { .. }) {
            ThreadMessage::JobResult { message }
        } else {
            ThreadMessage::PeerMessage { message }
        }
    }

    pub(super) fn task_subject(prompt: &str) -> String {
        let subject = prompt
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(str::trim)
            .unwrap_or("Task");
        const SUBJECT_LIMIT: usize = 120;
        let mut chars = subject.chars();
        let subject: String = chars.by_ref().take(SUBJECT_LIMIT).collect();
        if chars.next().is_some() {
            format!("{subject}...")
        } else {
            subject
        }
    }

    pub(super) async fn forward_job_result_to_runtime(
        &self,
        originating_thread_id: ThreadId,
        execution_thread_id: ThreadId,
        result: ThreadJobResult,
    ) {
        let mailbox_message = MailboxMessage {
            id: Uuid::new_v4().to_string(),
            from_thread_id: execution_thread_id,
            to_thread_id: originating_thread_id,
            from_label: result.agent_display_name.clone(),
            message_type: MailboxMessageType::JobResult {
                job_id: result.job_id.clone(),
                success: result.success,
                cancelled: result.cancelled,
                token_usage: result.token_usage.clone(),
                agent_id: result.agent_id,
                agent_display_name: result.agent_display_name.clone(),
                agent_description: result.agent_description.clone(),
            },
            text: result.message.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            summary: None,
        };
        let forwarder = self
            .chat_mailbox_forwarder
            .lock()
            .expect("chat mailbox forwarder mutex poisoned")
            .clone();
        let forwarded = match forwarder {
            Some(forwarder) => forwarder(originating_thread_id, mailbox_message.clone()).await,
            None => false,
        };
        if !forwarded {
            let _ = self
                .thread_pool
                .deliver_thread_message(
                    originating_thread_id,
                    Self::route_mailbox_message(mailbox_message),
                )
                .await;
        }
    }

    pub(super) fn broadcast_job_result(
        pipe_tx: &broadcast::Sender<ThreadEvent>,
        originating_thread_id: ThreadId,
        result: ThreadJobResult,
    ) {
        let _ = pipe_tx.send(ThreadEvent::JobResult {
            thread_id: originating_thread_id,
            job_id: result.job_id,
            success: result.success,
            cancelled: result.cancelled,
            message: result.message,
            token_usage: result.token_usage,
            agent_id: result.agent_id,
            agent_display_name: result.agent_display_name,
            agent_description: result.agent_description,
        });
    }
}
