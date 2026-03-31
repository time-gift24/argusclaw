//! PlanContinuationHook - auto-continue turn execution based on plan status.
//!
//! This hook inspects thread plan state at TurnEnd and can inject a follow-up
//! user message to continue execution when plan items remain unfinished.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use argus_protocol::{
    HookAction, HookEvent, HookHandler, PlanItemArg, StepStatus, ToolHookContext,
};
use async_trait::async_trait;

use crate::plan_store::FilePlanStore;

const CONTINUE_MESSAGE: &str = "请继续完成 plan 中剩余的任务，并在必要时更新 update_plan 的状态。";
const SUMMARY_MESSAGE: &str = "plan 所有步骤已完成，请总结目标是否已达成，并说明未达成项（如有）。";

/// TurnEnd hook that checks plan completion and optionally injects continuation messages.
pub struct PlanContinuationHook {
    store: Arc<FilePlanStore>,
    saw_incomplete: AtomicBool,
    sent_completion_check: AtomicBool,
}

impl PlanContinuationHook {
    #[must_use]
    pub fn new(store: Arc<FilePlanStore>) -> Self {
        Self {
            store,
            saw_incomplete: AtomicBool::new(false),
            sent_completion_check: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl HookHandler for PlanContinuationHook {
    async fn on_tool_event(&self, ctx: &ToolHookContext) -> HookAction {
        if ctx.event != HookEvent::TurnEnd {
            return HookAction::Continue;
        }

        let plan_snapshot = {
            let store = self.store.store();
            match store.read() {
                Ok(guard) => guard.clone(),
                Err(err) => {
                    tracing::warn!(error = %err, "PlanContinuationHook failed to read plan store");
                    return HookAction::Continue;
                }
            }
        };

        if plan_snapshot.is_empty() {
            return HookAction::Continue;
        }

        let mut has_incomplete = false;
        for raw in plan_snapshot {
            let item: PlanItemArg = match serde_json::from_value(raw) {
                Ok(item) => item,
                Err(err) => {
                    tracing::warn!(error = %err, "PlanContinuationHook found invalid plan item");
                    return HookAction::Continue;
                }
            };

            if matches!(item.status, StepStatus::Pending | StepStatus::InProgress) {
                has_incomplete = true;
                break;
            }
        }

        if has_incomplete {
            self.saw_incomplete.store(true, Ordering::Relaxed);
            return HookAction::ContinueWithMessage(CONTINUE_MESSAGE.to_string());
        }

        if self.saw_incomplete.load(Ordering::Relaxed)
            && !self.sent_completion_check.swap(true, Ordering::Relaxed)
        {
            return HookAction::ContinueWithMessage(SUMMARY_MESSAGE.to_string());
        }

        HookAction::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    fn make_store() -> Arc<FilePlanStore> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Arc::new(FilePlanStore::new(
            std::env::temp_dir().join("argus-plan-hook-tests"),
            &format!("thread-{nanos}"),
        ))
    }

    fn turn_end_ctx() -> ToolHookContext {
        ToolHookContext {
            event: HookEvent::TurnEnd,
            tool_name: String::new(),
            tool_call_id: String::new(),
            tool_input: serde_json::Value::Null,
            tool_result: None,
            error: None,
            tool_manager: None,
            thread_event_sender: None,
            thread_id: None,
            turn_number: None,
        }
    }

    #[tokio::test]
    async fn empty_plan_returns_continue() {
        let store = make_store();
        let hook = PlanContinuationHook::new(store);
        let action = hook.on_tool_event(&turn_end_ctx()).await;
        assert!(matches!(action, HookAction::Continue));
    }

    #[tokio::test]
    async fn pending_or_in_progress_returns_continue_with_message() {
        let store = make_store();
        store.write_from_items(vec![PlanItemArg {
            step: "step 1".to_string(),
            status: StepStatus::Pending,
        }]);
        let hook = PlanContinuationHook::new(store);
        let action = hook.on_tool_event(&turn_end_ctx()).await;
        assert!(matches!(action, HookAction::ContinueWithMessage(_)));
    }

    #[tokio::test]
    async fn all_completed_without_prior_incomplete_returns_continue() {
        let store = make_store();
        store.write_from_items(vec![PlanItemArg {
            step: "step 1".to_string(),
            status: StepStatus::Completed,
        }]);
        let hook = PlanContinuationHook::new(store);
        let action = hook.on_tool_event(&turn_end_ctx()).await;
        assert!(matches!(action, HookAction::Continue));
    }

    #[tokio::test]
    async fn incomplete_then_completed_emits_summary_once() {
        let store = make_store();
        let hook = PlanContinuationHook::new(store.clone());

        store.write_from_items(vec![PlanItemArg {
            step: "step 1".to_string(),
            status: StepStatus::InProgress,
        }]);
        let action_1 = hook.on_tool_event(&turn_end_ctx()).await;
        assert!(matches!(action_1, HookAction::ContinueWithMessage(_)));

        store.write_from_items(vec![PlanItemArg {
            step: "step 1".to_string(),
            status: StepStatus::Completed,
        }]);
        let action_2 = hook.on_tool_event(&turn_end_ctx()).await;
        assert!(matches!(action_2, HookAction::ContinueWithMessage(_)));

        let action_3 = hook.on_tool_event(&turn_end_ctx()).await;
        assert!(matches!(action_3, HookAction::Continue));
    }

    #[tokio::test]
    async fn poisoned_rwlock_returns_continue() {
        let store = make_store();
        let lock = store.store();
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _guard = lock.write().unwrap();
            panic!("poison lock");
        }));

        let hook = PlanContinuationHook::new(store);
        let action = hook.on_tool_event(&turn_end_ctx()).await;
        assert!(matches!(action, HookAction::Continue));
    }

    #[tokio::test]
    async fn invalid_plan_item_returns_continue() {
        let store = make_store();
        {
            let lock = store.store();
            let mut guard = lock.write().unwrap();
            *guard = vec![json!({
                "unexpected": "shape",
            })];
        }

        let hook = PlanContinuationHook::new(store);
        let action = hook.on_tool_event(&turn_end_ctx()).await;
        assert!(matches!(action, HookAction::Continue));
    }
}
