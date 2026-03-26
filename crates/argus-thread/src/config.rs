//! Thread configuration.

use derive_builder::Builder;

use argus_turn::TurnConfig;

/// Thread configuration.
#[derive(Debug, Clone, Builder)]
pub struct ThreadConfig {
    /// Token threshold ratio to trigger pre-turn compaction (e.g., 0.8 = 80% of context window).
    /// This currently acts as a thread-level threshold override for the built-in compactors.
    #[builder(default = 0.8)]
    pub compact_threshold_ratio: f32,

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
    fn thread_config_default() {
        let config = ThreadConfig::default();
        assert!((config.compact_threshold_ratio - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn thread_config_builder_custom() {
        let config = ThreadConfigBuilder::default()
            .compact_threshold_ratio(0.9)
            .build()
            .unwrap();

        assert!((config.compact_threshold_ratio - 0.9).abs() < f32::EPSILON);
    }
}
