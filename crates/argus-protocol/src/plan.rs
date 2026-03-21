//! Plan types for the update_plan tool.
//!
//! Shared types for plan serialization/deserialization between argus-thread and
//! external consumers (UI reads plan from Thread).

use serde::{Deserialize, Serialize};

/// Status of a plan step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// Step has not been started.
    Pending,
    /// Step is currently being worked on.
    InProgress,
    /// Step has been completed.
    Completed,
}

/// A single item in a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanItemArg {
    /// The step description.
    pub step: String,
    /// The current status of this step.
    pub status: StepStatus,
}

/// Arguments for the `update_plan` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePlanArgs {
    /// Optional explanation for the plan update (logged, not stored).
    #[serde(default)]
    pub explanation: Option<String>,
    /// The full plan snapshot. The LLM sends the complete plan on each call.
    pub plan: Vec<PlanItemArg>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_status_serialization() {
        assert_eq!(
            serde_json::to_string(&StepStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&StepStatus::InProgress).unwrap(),
            "\"in_progress\""
        );
        assert_eq!(
            serde_json::to_string(&StepStatus::Completed).unwrap(),
            "\"completed\""
        );
    }

    #[test]
    fn step_status_deserialization() {
        assert_eq!(
            serde_json::from_str::<StepStatus>("\"pending\"").unwrap(),
            StepStatus::Pending
        );
        assert_eq!(
            serde_json::from_str::<StepStatus>("\"in_progress\"").unwrap(),
            StepStatus::InProgress
        );
        assert_eq!(
            serde_json::from_str::<StepStatus>("\"completed\"").unwrap(),
            StepStatus::Completed
        );
    }

    #[test]
    fn plan_item_arg_roundtrip() {
        let item = PlanItemArg {
            step: "Implement feature X".to_string(),
            status: StepStatus::InProgress,
        };
        let json = serde_json::to_string(&item).unwrap();
        let parsed: PlanItemArg = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.step, item.step);
        assert_eq!(parsed.status, item.status);
    }

    #[test]
    fn update_plan_args_roundtrip() {
        let args = UpdatePlanArgs {
            explanation: Some("Finished step 1".to_string()),
            plan: vec![
                PlanItemArg {
                    step: "Step 1".to_string(),
                    status: StepStatus::Completed,
                },
                PlanItemArg {
                    step: "Step 2".to_string(),
                    status: StepStatus::Pending,
                },
            ],
        };
        let json = serde_json::to_string(&args).unwrap();
        let parsed: UpdatePlanArgs = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.explanation, args.explanation);
        assert_eq!(parsed.plan.len(), 2);
        assert_eq!(parsed.plan[0].step, "Step 1");
        assert_eq!(parsed.plan[1].status, StepStatus::Pending);
    }

    #[test]
    fn update_plan_args_without_explanation() {
        let args = UpdatePlanArgs {
            explanation: None,
            plan: vec![],
        };
        let json = serde_json::to_string(&args).unwrap();
        let parsed: UpdatePlanArgs = serde_json::from_str(&json).unwrap();
        assert!(parsed.explanation.is_none());
        assert!(parsed.plan.is_empty());
    }

    #[test]
    fn plan_item_arg_unknown_field_rejected() {
        let json = r#"{"step":"Test","status":"pending","extra":"field"}"#;
        let result: Result<PlanItemArg, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
