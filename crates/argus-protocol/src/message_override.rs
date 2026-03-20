//! Message-level configuration override.

use serde::{Deserialize, Serialize};

use super::llm::ThinkingConfig;

/// Override parameters for a single message send.
/// These override the agent's default configuration for one request only.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageOverride {
    /// Override max_tokens for this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Override temperature for this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Override thinking_config for this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<ThinkingConfig>,
}
