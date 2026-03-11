//! Thread configuration.

use std::sync::Arc;

use derive_builder::Builder;

use crate::agents::turn::TurnConfig;
use crate::llm::LlmProvider;

/// Strategy for compacting thread context.
pub enum CompactStrategy {
    /// Keep the most recent N messages.
    KeepRecent {
        /// Number of recent messages to keep.
        count: usize,
    },
    /// Keep messages within N% of token budget.
    KeepTokens {
        /// Ratio of tokens to keep (0.0 - 1.0).
        ratio: f32,
    },
    /// Use LLM to summarize history.
    Summarize {
        /// Maximum tokens for the summary.
        max_summary_tokens: u32,
        /// Provider to use for summarization.
        provider: Arc<dyn LlmProvider>,
    },
}

impl std::fmt::Debug for CompactStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeepRecent { count } => f.debug_struct("KeepRecent").field("count", count).finish(),
            Self::KeepTokens { ratio } => f.debug_struct("KeepTokens").field("ratio", ratio).finish(),
            Self::Summarize { max_summary_tokens, .. } => f
                .debug_struct("Summarize")
                .field("max_summary_tokens", max_summary_tokens)
                .field("provider", &"Arc<dyn LlmProvider>")
                .finish(),
        }
    }
}

impl Clone for CompactStrategy {
    fn clone(&self) -> Self {
        match self {
            Self::KeepRecent { count } => Self::KeepRecent { count: *count },
            Self::KeepTokens { ratio } => Self::KeepTokens { ratio: *ratio },
            Self::Summarize { max_summary_tokens, provider } => Self::Summarize {
                max_summary_tokens: *max_summary_tokens,
                provider: provider.clone(),
            },
        }
    }
}

impl Default for CompactStrategy {
    fn default() -> Self {
        Self::KeepRecent { count: 50 }
    }
}

/// Thread configuration.
#[derive(Debug, Clone, Builder)]
pub struct ThreadConfig {
    /// Token threshold ratio to trigger compact (e.g., 0.8 = 80% of context window).
    #[builder(default = 0.8)]
    pub compact_threshold_ratio: f32,

    /// Strategy for compacting context.
    #[builder(default)]
    pub compact_strategy: CompactStrategy,

    /// Underlying Turn configuration.
    #[builder(default)]
    pub turn_config: TurnConfig,
}

impl Default for ThreadConfig {
    fn default() -> Self {
        ThreadConfigBuilder::default()
            .build()
            .expect("ThreadConfigBuilder should not fail with defaults")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_strategy_default_is_keep_recent() {
        match CompactStrategy::default() {
            CompactStrategy::KeepRecent { count } => assert_eq!(count, 50),
            _ => panic!("Expected KeepRecent strategy"),
        }
    }

    #[test]
    fn thread_config_default() {
        let config = ThreadConfig::default();
        assert!((config.compact_threshold_ratio - 0.8).abs() < f32::EPSILON);
        assert!(matches!(
            config.compact_strategy,
            CompactStrategy::KeepRecent { count: 50 }
        ));
    }

    #[test]
    fn thread_config_builder_custom() {
        let config = ThreadConfigBuilder::default()
            .compact_threshold_ratio(0.9)
            .compact_strategy(CompactStrategy::KeepRecent { count: 100 })
            .build()
            .unwrap();

        assert!((config.compact_threshold_ratio - 0.9).abs() < f32::EPSILON);
        match config.compact_strategy {
            CompactStrategy::KeepRecent { count } => assert_eq!(count, 100),
            _ => panic!("Expected KeepRecent strategy"),
        }
    }
}
