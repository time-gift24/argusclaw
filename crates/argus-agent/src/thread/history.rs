use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::command::ThreadRuntimeSnapshot;
use crate::history::{CompactionCheckpoint, TurnRecord, flatten_turn_messages, shared_history};
use crate::turn_log_store::RecoveredThreadLogState;
use argus_protocol::llm::{ChatMessage, Role};

use super::Thread;

impl Thread {
    pub(super) fn build_turn_context(&self) -> Arc<Vec<ChatMessage>> {
        if let Some(checkpoint) = self.compaction_checkpoint.as_ref() {
            let mut context_messages = self.system_messages.clone();
            context_messages.extend(checkpoint.summary_messages.iter().cloned());
            context_messages.extend(
                self.turns
                    .iter()
                    .filter(|turn| turn.turn_number > checkpoint.summarized_through_turn)
                    .flat_map(|turn| turn.messages.iter().cloned()),
            );
            Arc::new(context_messages)
        } else {
            Arc::clone(shared_history(
                &self.messages,
                self.cached_committed_messages.as_ref(),
            ))
        }
    }

    /// Hydrate thread runtime state from persisted history.
    pub fn hydrate_from_persisted_state(
        &mut self,
        mut messages: Vec<ChatMessage>,
        token_count: u32,
        turn_count: u32,
        updated_at: DateTime<Utc>,
    ) {
        let existing_system = self
            .messages
            .first()
            .filter(|message| message.role == Role::System)
            .cloned();
        let has_system_message = messages
            .first()
            .is_some_and(|message| message.role == Role::System);

        if !has_system_message && let Some(system_message) = existing_system {
            messages.insert(0, system_message);
        }

        self.messages = Arc::new(messages);
        self.system_messages = Self::collect_system_messages(self.messages.as_slice());
        self.turns.clear();
        self.current_turn = None;
        self.compaction_checkpoint = None;
        self.cached_committed_messages = Some(Arc::clone(&self.messages));
        self.token_count = token_count;
        self.turn_count = turn_count;
        self.next_turn_number = turn_count.saturating_add(1);
        self.updated_at = updated_at;
    }

    pub fn hydrate_from_turn_log_state(
        &mut self,
        recovered: RecoveredThreadLogState,
        updated_at: DateTime<Utc>,
    ) {
        let token_count = recovered.token_count();
        let turn_count = recovered.turn_count();
        let RecoveredThreadLogState {
            system_messages,
            turns,
            checkpoint,
        } = recovered;
        self.restore_committed_turn_history(
            self.restore_system_messages(system_messages),
            turns,
            checkpoint,
        );
        self.token_count = token_count;
        self.turn_count = turn_count;
        self.next_turn_number = Self::derive_next_turn_number(&self.turns);
        self.active_turn_cancellation = None;
        self.runtime_snapshot = ThreadRuntimeSnapshot::default();
        self.updated_at = updated_at;
    }

    pub(super) fn collect_system_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
        messages
            .iter()
            .take_while(|message| message.role == Role::System)
            .cloned()
            .collect()
    }

    pub(super) fn sync_cached_history_from_flat_messages(&mut self) {
        self.system_messages = Self::collect_system_messages(self.messages.as_slice());
        self.cached_committed_messages = Some(Arc::clone(&self.messages));
    }

    pub(super) fn materialize_committed_messages(
        system_messages: &[ChatMessage],
        turns: &[TurnRecord],
    ) -> Arc<Vec<ChatMessage>> {
        let mut messages = system_messages.to_vec();
        messages.extend(flatten_turn_messages(turns));
        Arc::new(messages)
    }

    pub(super) fn latest_turn_number(turns: &[TurnRecord]) -> u32 {
        turns.iter().map(|turn| turn.turn_number).max().unwrap_or(0)
    }

    pub(super) fn derive_next_turn_number(turns: &[TurnRecord]) -> u32 {
        Self::latest_turn_number(turns).saturating_add(1).max(1)
    }

    pub(super) fn sync_turn_counters(&mut self, turn_number: u32) {
        self.next_turn_number = self.next_turn_number.max(turn_number.saturating_add(1));
        self.turn_count = self.turn_count.max(turn_number);
    }

    pub(super) fn restore_system_messages(
        &self,
        mut system_messages: Vec<ChatMessage>,
    ) -> Vec<ChatMessage> {
        if system_messages.is_empty()
            && let Some(system_message) = self
                .messages
                .first()
                .filter(|message| message.role == Role::System)
                .cloned()
        {
            system_messages.push(system_message);
        }

        system_messages
    }

    pub(super) fn restore_committed_turn_history(
        &mut self,
        system_messages: Vec<ChatMessage>,
        turns: Vec<TurnRecord>,
        checkpoint: Option<CompactionCheckpoint>,
    ) {
        self.system_messages = system_messages;
        self.turns = turns;
        self.current_turn = None;
        self.compaction_checkpoint = checkpoint;
        let committed_messages =
            Self::materialize_committed_messages(&self.system_messages, &self.turns);
        self.cached_committed_messages = Some(Arc::clone(&committed_messages));
        self.messages = committed_messages;
    }

    #[cfg(test)]
    pub(super) fn hydrate_turn_history_for_test(&mut self, turns: Vec<TurnRecord>) {
        self.restore_committed_turn_history(self.system_messages.clone(), turns, None);
        self.turn_count = Self::latest_turn_number(&self.turns);
        self.next_turn_number = Self::derive_next_turn_number(&self.turns);
    }
}
