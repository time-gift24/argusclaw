//! Sleep tool implementation for short asynchronous waits.

use std::sync::Arc;
use std::time::Duration;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{ToolOutputError, serialize_tool_output};

const MIN_DURATION_MS: u64 = 1;
const MAX_DURATION_MS: u64 = 120_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SleepArgs {
    duration_ms: u64,
}

#[derive(Debug, Serialize)]
struct SleepResponse {
    slept_ms: u64,
}

#[derive(Debug, thiserror::Error)]
enum SleepToolError {
    #[error("Invalid arguments: {0}")]
    InvalidArguments(#[from] serde_json::Error),
    #[error("duration_ms must be between {MIN_DURATION_MS} and {MAX_DURATION_MS}")]
    DurationOutOfRange,
    #[error(transparent)]
    Output(#[from] ToolOutputError),
}

impl From<SleepToolError> for ToolError {
    fn from(error: SleepToolError) -> Self {
        ToolError::ExecutionFailed {
            tool_name: SleepTool::TOOL_NAME.to_string(),
            reason: error.to_string(),
        }
    }
}

/// Tool for pausing the current turn for a bounded duration.
pub struct SleepTool;

impl Default for SleepTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SleepTool {
    const TOOL_NAME: &'static str = "sleep";

    /// Create a new SleepTool.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    async fn execute_impl(
        &self,
        input: serde_json::Value,
    ) -> Result<SleepResponse, SleepToolError> {
        let args: SleepArgs = serde_json::from_value(input)?;
        if !(MIN_DURATION_MS..=MAX_DURATION_MS).contains(&args.duration_ms) {
            return Err(SleepToolError::DurationOutOfRange);
        }

        tokio::time::sleep(Duration::from_millis(args.duration_ms)).await;
        Ok(SleepResponse {
            slept_ms: args.duration_ms,
        })
    }
}

#[async_trait]
impl NamedTool for SleepTool {
    fn name(&self) -> &str {
        Self::TOOL_NAME
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::TOOL_NAME.to_string(),
            description: "Wait for a bounded number of milliseconds before continuing.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "duration_ms": {
                        "type": "integer",
                        "minimum": MIN_DURATION_MS,
                        "maximum": MAX_DURATION_MS,
                        "description": "Milliseconds to wait before continuing. Must be between 1 and 120000."
                    }
                },
                "required": ["duration_ms"],
                "additionalProperties": false
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let response = self.execute_impl(input).await.map_err(ToolError::from)?;
        serialize_tool_output(Self::TOOL_NAME, response)
            .map_err(SleepToolError::from)
            .map_err(ToolError::from)
    }
}
