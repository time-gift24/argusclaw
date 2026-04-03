//! Backward-compatible shim for `TokenUsage`.

pub use crate::llm::TokenUsage;

#[cfg(test)]
mod tests {
    use super::TokenUsage;

    #[test]
    fn test_token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn test_token_usage_equality() {
        let usage1 = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let usage2 = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let usage3 = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 200,
        };
        assert_eq!(usage1, usage2);
        assert_ne!(usage1, usage3);
    }

    #[test]
    fn test_token_usage_new_sets_total() {
        let usage = TokenUsage::new(18, 2428);

        assert_eq!(usage.prompt_tokens, 18);
        assert_eq!(usage.completion_tokens, 2428);
        assert_eq!(usage.total_tokens, 2446);
    }

    #[test]
    fn test_token_usage_deserializes_legacy_shape() {
        let usage: TokenUsage =
            serde_json::from_str(r#"{"input_tokens":18,"output_tokens":2428,"total_tokens":2446}"#)
                .expect("legacy usage payload should deserialize");

        assert_eq!(usage.prompt_tokens, 18);
        assert_eq!(usage.completion_tokens, 2428);
        assert_eq!(usage.total_tokens, 2446);
    }
}
