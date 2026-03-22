//! Token usage statistics.

use serde::{Deserialize, Serialize};

/// Token usage statistics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Number of input tokens used.
    pub input_tokens: u32,
    /// Number of output tokens generated.
    pub output_tokens: u32,
    /// Total tokens (input + output).
    pub total_tokens: u32,
    /// Number of reasoning tokens (for models like o1, o3, GLM with thinking).
    /// These tokens don't count against the context window.
    #[serde(default)]
    pub reasoning_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
        assert_eq!(usage.reasoning_tokens, 0);
    }

    #[test]
    fn test_token_usage_equality() {
        let usage1 = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            reasoning_tokens: 0,
        };
        let usage2 = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            reasoning_tokens: 0,
        };
        let usage3 = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 200,
            reasoning_tokens: 0,
        };
        assert_eq!(usage1, usage2);
        assert_ne!(usage1, usage3);
    }

    #[test]
    fn test_token_usage_with_reasoning() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            reasoning_tokens: 200,
        };
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
        assert_eq!(usage.reasoning_tokens, 200);
    }
}
