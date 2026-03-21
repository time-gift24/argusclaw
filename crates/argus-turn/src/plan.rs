//! Plan generation and execution for Turn-level action planning.
//!
//! When `plan_enabled` is set on `TurnConfig`, the Turn generates a structured
//! `ActionPlan` at the start of execution, then executes the plan step-by-step
//! before falling back to normal LLM-driven tool selection.

use serde::{Deserialize, Serialize};
use tracing;

use argus_protocol::llm::{ChatMessage, CompletionRequest, LlmProvider, ToolDefinition};

use crate::error::TurnError;

/// A planned action to take.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedAction {
    /// Tool to use.
    pub tool_name: String,
    /// Parameters for the tool.
    pub parameters: serde_json::Value,
    /// Reasoning for this action.
    pub reasoning: String,
    /// Expected outcome.
    pub expected_outcome: String,
}

/// Result of planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPlan {
    /// Overall goal understanding.
    pub goal: String,
    /// Planned sequence of actions.
    pub actions: Vec<PlannedAction>,
    /// Estimated total cost.
    pub estimated_cost: Option<f64>,
    /// Estimated total time in seconds.
    pub estimated_time_secs: Option<u64>,
    /// Confidence in the plan (0-1).
    pub confidence: f64,
}

/// Context for planning.
pub struct PlannerContext<'a> {
    /// Historical messages for the conversation.
    pub messages: &'a [ChatMessage],
    /// Available tool definitions.
    pub available_tools: Vec<ToolDefinition>,
    /// Optional job description.
    pub job_description: Option<String>,
}

/// Outcome of plan execution.
#[derive(Debug)]
pub enum PlanOutcome {
    /// Plan executed successfully and job is complete.
    Completed,
    /// Plan executed but job needs more work (fall back to normal loop).
    NeedsMoreWork,
    /// Plan interrupted by user message.
    Interrupted,
}

/// Extract JSON from text that might contain other content.
pub fn extract_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if start < end {
        Some(&text[start..=end])
    } else {
        None
    }
}

/// Strip paired XML-style tags and their content from text.
///
/// Strips paired tags (e.g. `<thinking>...</thinking>`) together, so closing
/// tags don't get processed as standalone tags that corrupt the string.
fn strip_paired_tags(text: &str, open_tag: &str, close_tag: &str) -> String {
    let mut text_lower = text.to_lowercase();
    let open_lower = open_tag.to_lowercase();
    let close_lower = close_tag.to_lowercase();

    let mut result = text.to_string();

    while let Some(open_pos) = text_lower.find(&open_lower) {
        // Check if this is inside an already-removed region (no, we process sequentially)
        if let Some(close_pos) = text_lower[open_pos..].find(&close_lower) {
            let close_abs = open_pos + close_pos;
            let close_end = close_abs + close_tag.len();
            result = format!("{}{}", &result[..open_pos], &result[close_end..]);
            text_lower = result.to_lowercase();
            // Next iteration searches from 0 again (positions have shifted)
        } else {
            // No matching close tag — stop searching for this tag
            break;
        }
    }

    result
}

/// Clean reasoning model artifacts from LLM response before JSON parsing.
///
/// Strips XML-style tool tags, thinking tags, and other artifacts that could
/// interfere with JSON parsing.
pub fn clean_response(text: &str) -> String {
    let mut result = text.trim().to_string();

    // Strip paired tags: define (open_tag, close_tag) pairs.
    // Process open_tag first, then find and strip its matching close_tag together.
    // This prevents standalone closing tags like `</thinking>` from being processed
    // before their opening counterparts, which would corrupt the string.
    let paired_tags = [
        ("<invoke name=", "</invoke>"),
        ("<thinking>", "</thinking>"),
        ("<plan>", "</plan>"),
        ("<final>", "</final>"),
        ("<result>", "</result>"),
        ("<output>", "</output>"),
    ];

    for (open_tag, close_tag) in &paired_tags {
        result = strip_paired_tags(&result, open_tag, close_tag);
    }

    result = result.trim().to_string();

    if let Some(start) = result.find("[Called tool `") {
        result = result[..start].to_string();
    }

    let truncation_markers = [
        "I'll",
        "Let me",
        "I need to",
        "First, let me",
        "First I'll",
        "Let me first",
        "I'll start by",
    ];
    for marker in &truncation_markers {
        if let Some(pos) = result.find(marker) {
            let before = &result[..pos];
            let json_count = before.matches('{').count();
            let close_count = before.matches('}').count();
            if json_count > 0 && json_count == close_count {
                result = before.to_string();
            }
        }
    }

    result
}

/// Planner — generates ActionPlan via LLM call.
pub struct Planner<'a> {
    provider: &'a dyn LlmProvider,
}

impl<'a> Planner<'a> {
    /// Create a new Planner.
    pub fn new(provider: &'a dyn LlmProvider) -> Self {
        Self { provider }
    }
    /// Generate a plan for completing a goal.
    pub async fn plan(&self, ctx: &PlannerContext<'_>) -> Result<ActionPlan, TurnError> {
        let planning_prompt = self.build_planning_prompt(ctx);

        let mut messages: Vec<ChatMessage> = ctx.messages.to_vec();
        messages.push(ChatMessage::user(planning_prompt));

        let request = CompletionRequest::new(messages)
            .with_max_tokens(2048)
            .with_temperature(0.3);

        tracing::debug!("Generating action plan via LLM");
        let response = self
            .provider
            .complete(request)
            .await
            .map_err(|e| TurnError::PlanGenerationFailed(format!("LLM call failed: {}", e)))?;

        let cleaned = clean_response(&response.content);
        self.parse_plan(&cleaned)
    }

    /// Ask LLM if the job is complete.
    pub async fn is_complete(&self, messages: &mut Vec<ChatMessage>) -> Result<bool, TurnError> {
        let completion_prompt = "Based on the conversation above, has the user's request or job been fully completed? \
            Respond with a JSON object in this format: {\"complete\": true/false, \"reasoning\": \"brief explanation\"}";
        messages.push(ChatMessage::user(completion_prompt));

        let request = CompletionRequest::new(messages.clone())
            .with_max_tokens(256)
            .with_temperature(0.1);

        let response = self.provider.complete(request).await.map_err(|e| {
            TurnError::PlanGenerationFailed(format!("LLM completion check failed: {}", e))
        })?;

        let json_str = extract_json(&response.content).unwrap_or(&response.content);

        #[derive(Deserialize)]
        struct CompletionCheck {
            complete: bool,
        }

        match serde_json::from_str::<CompletionCheck>(json_str) {
            Ok(check) => {
                tracing::debug!(complete = check.complete, "Plan completion check result");
                Ok(check.complete)
            }
            Err(_) => {
                tracing::warn!("Could not parse completion check response, assuming not complete");
                Ok(false)
            }
        }
    }

    /// Build the planning system prompt with available tools.
    fn build_planning_prompt(&self, ctx: &PlannerContext<'_>) -> String {
        let tools_desc = if ctx.available_tools.is_empty() {
            "No tools available.".to_string()
        } else {
            ctx.available_tools
                .iter()
                .map(|t| format!("- {}: {}", t.name, t.description))
                .collect::<Vec<_>>()
                .join("\n")
        };

        format!(
            r#"You are a planning assistant for an autonomous agent. Your job is to create detailed, actionable plans.

Available tools:
{tools_desc}

When creating a plan:
1. Break down the goal into specific, achievable steps
2. Select the most appropriate tool for each step
3. Consider dependencies between steps
4. Estimate costs and time realistically
5. Identify potential failure points

Respond with a JSON plan in this format:
{{
    "goal": "Clear statement of the goal",
    "actions": [
        {{
            "tool_name": "tool_to_use",
            "parameters": {{}},
            "reasoning": "Why this action",
            "expected_outcome": "What should happen"
        }}
    ],
    "estimated_cost": 0.0,
    "estimated_time_secs": 0,
    "confidence": 0.0-1.0
}}"#
        )
    }

    /// Parse a plan from LLM response content.
    fn parse_plan(&self, content: &str) -> Result<ActionPlan, TurnError> {
        let json_str = extract_json(content).unwrap_or(content);

        serde_json::from_str(json_str).map_err(|e| {
            TurnError::PlanParseFailed(format!("Failed to parse plan from LLM response: {}", e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_simple() {
        let json = r#"{"goal": "test", "actions": [], "confidence": 1.0}"#;
        assert!(extract_json(json).is_some());
    }

    #[test]
    fn test_extract_json_with_prefix() {
        let text = "Here is the plan:\n{\"goal\": \"test\", \"actions\": [], \"confidence\": 1.0}";
        let extracted = extract_json(text);
        assert!(extracted.is_some());
        let s = extracted.unwrap();
        assert!(s.starts_with('{'));
        assert!(s.ends_with('}'));
    }

    #[test]
    fn test_extract_json_empty() {
        assert!(extract_json("no json here").is_none());
    }

    #[test]
    fn test_clean_response_simple() {
        let input = r#"{"goal": "test", "actions": [], "confidence": 1.0}"#;
        let cleaned = clean_response(input);
        assert!(cleaned.contains("goal"));
    }

    #[test]
    fn test_clean_response_with_thinking_tags() {
        let input = r#"<thinking>I need to plan this carefully</thinking>
{"goal": "test", "actions": [], "confidence": 1.0}
<final>Done</final>"#;
        let cleaned = clean_response(input);
        assert!(cleaned.contains("goal"));
        assert!(!cleaned.contains("thinking"));
        assert!(!cleaned.contains("final"));
    }

    #[test]
    fn test_clean_response_strips_continuation() {
        let input = r#"{"goal": "test", "actions": [], "confidence": 1.0}
Let me explain the plan..."#;
        let cleaned = clean_response(input);
        assert!(cleaned.contains("goal"));
        assert!(!cleaned.contains("Let me explain"));
    }

    #[test]
    fn test_plan_outcome_debug() {
        assert!(format!("{:?}", PlanOutcome::Completed).contains("Completed"));
        assert!(format!("{:?}", PlanOutcome::NeedsMoreWork).contains("NeedsMoreWork"));
        assert!(format!("{:?}", PlanOutcome::Interrupted).contains("Interrupted"));
    }

    #[test]
    fn test_action_plan_deserialize() {
        let json = r#"{
            "goal": "Test goal",
            "actions": [
                {
                    "tool_name": "echo",
                    "parameters": {"msg": "hello"},
                    "reasoning": "Test action",
                    "expected_outcome": "Hello echoed"
                }
            ],
            "estimated_cost": 1.5,
            "estimated_time_secs": 10,
            "confidence": 0.9
        }"#;
        let plan: ActionPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.goal, "Test goal");
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].tool_name, "echo");
        assert_eq!(plan.confidence, 0.9);
        assert_eq!(plan.estimated_cost, Some(1.5));
        assert_eq!(plan.estimated_time_secs, Some(10));
    }

    #[test]
    fn test_action_plan_deserialize_minimal() {
        let json = r#"{"goal": "Test", "actions": [], "confidence": 0.5}"#;
        let plan: ActionPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.goal, "Test");
        assert!(plan.actions.is_empty());
        assert_eq!(plan.confidence, 0.5);
        assert!(plan.estimated_cost.is_none());
        assert!(plan.estimated_time_secs.is_none());
    }
}
