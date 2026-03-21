//! Token usage statistics and tokenization.

use serde::{Deserialize, Serialize};
use tiktoken::CoreBpe;

use crate::llm::{ChatMessage, Role};

/// Token usage statistics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Number of input tokens used.
    pub input_tokens: u32,
    /// Number of output tokens generated.
    pub output_tokens: u32,
    /// Total tokens (input + output).
    pub total_tokens: u32,
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// Tokenizer error types.
#[derive(Debug, thiserror::Error)]
pub enum TokenizerError {
    #[error("failed to initialize tiktoken encoder: {0}")]
    Init(String),
}

/// Trait for tokenizers that can estimate or compute token counts.
pub trait Tokenizer: Send + Sync {
    /// Estimate token count for a string.
    fn encode(&self, text: &str) -> usize;

    /// Estimate token count for a ChatMessage.
    fn encode_message(&self, msg: &ChatMessage) -> usize;
}

/// Tiktoken-based tokenizer using the cl100k_base encoding.
///
/// This is the standard encoding used by most modern LLMs including GPT-4,
/// Claude, and GLM models.
pub struct TiktokenTokenizer {
    encoder: &'static CoreBpe,
}

impl TiktokenTokenizer {
    /// Create a new TiktokenTokenizer using cl100k_base encoding.
    pub fn new() -> Result<Self, TokenizerError> {
        let encoder = tiktoken::get_encoding("cl100k_base")
            .ok_or_else(|| TokenizerError::Init("cl100k_base encoding not found".to_string()))?;
        Ok(Self { encoder })
    }
}

impl Tokenizer for TiktokenTokenizer {
    fn encode(&self, text: &str) -> usize {
        self.encoder.count_with_special_tokens(text)
    }

    fn encode_message(&self, msg: &ChatMessage) -> usize {
        let role_str = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };

        // Estimate the role prefix token count by encoding a template.
        // cl100k_base encodes JSON field names efficiently, so we include
        // the minimal JSON structure overhead.
        let role_prefix_tokens =
            self.encode(&format!(r#"{{"role":"{}","content":""#, role_str));
        let content_tokens = self.encode(&msg.content);

        let mut total = role_prefix_tokens + content_tokens + 3; // closing braces + comma

        // Add overhead for optional fields when present.
        if msg.reasoning_content.is_some() {
            total += self.encode(r#","reasoning_content":""#) + 3;
            if let Some(ref rc) = msg.reasoning_content {
                total += self.encode(rc);
            }
        }

        if msg.tool_call_id.is_some() {
            total += self.encode(r#","tool_call_id":""#) + 3;
            if let Some(ref id) = msg.tool_call_id {
                total += self.encode(id);
            }
        }

        if msg.name.is_some() {
            total += self.encode(r#","name":""#) + 3;
            if let Some(ref name) = msg.name {
                total += self.encode(name);
            }
        }

        if msg.tool_calls.is_some() {
            total += self.encode(r#","tool_calls":""#);
            // Approximate: tool_calls adds significant overhead, estimate ~50 tokens base
            total += 50;
        }

        if !msg.content_parts.is_empty() {
            // Content parts use array format, estimate overhead
            total += self.encode(r#","content":["#) + 2; // approximate for closing ]
        }

        total
    }
}

/// Global tiktoken tokenizer instance for use in token estimation.
static TIKTOKEN: std::sync::OnceLock<TiktokenTokenizer> = std::sync::OnceLock::new();

/// Get the global TiktokenTokenizer instance.
pub fn tiktoken() -> &'static TiktokenTokenizer {
    TIKTOKEN.get_or_init(|| TiktokenTokenizer::new().expect("failed to initialize tiktoken tokenizer"))
}

/// Estimate token count for a string using the global tiktoken tokenizer.
pub fn estimate_tokens(content: &str) -> usize {
    tiktoken().encode(content)
}

/// Estimate token count for a ChatMessage using the global tiktoken tokenizer.
pub fn estimate_message_tokens(msg: &ChatMessage) -> usize {
    tiktoken().encode_message(msg)
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
    }

    #[test]
    fn test_token_usage_equality() {
        let usage1 = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        };
        let usage2 = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        };
        let usage3 = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 200,
        };
        assert_eq!(usage1, usage2);
        assert_ne!(usage1, usage3);
    }
}
