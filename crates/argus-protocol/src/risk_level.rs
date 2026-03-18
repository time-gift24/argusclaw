//! Risk level classification for operations requiring approval.

use serde::{Deserialize, Serialize};

/// Risk level of an operation requiring approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    /// Returns a warning emoji suitable for display in dashboards and chat.
    pub fn emoji(&self) -> &'static str {
        match self {
            RiskLevel::Low => "\u{2139}\u{fe0f}",      // information source
            RiskLevel::Medium => "\u{26a0}\u{fe0f}",   // warning sign
            RiskLevel::High => "\u{1f6a8}",            // rotating light
            RiskLevel::Critical => "\u{2620}\u{fe0f}", // skull and crossbones
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_level_emoji() {
        assert_eq!(RiskLevel::Low.emoji(), "\u{2139}\u{fe0f}");
        assert_eq!(RiskLevel::Medium.emoji(), "\u{26a0}\u{fe0f}");
        assert_eq!(RiskLevel::High.emoji(), "\u{1f6a8}");
        assert_eq!(RiskLevel::Critical.emoji(), "\u{2620}\u{fe0f}");
    }

    #[test]
    fn risk_level_serde_roundtrip() {
        for level in [
            RiskLevel::Low,
            RiskLevel::Medium,
            RiskLevel::High,
            RiskLevel::Critical,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let back: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, back);
        }
    }

    #[test]
    fn risk_level_rename_all() {
        let json = serde_json::to_string(&RiskLevel::Critical).unwrap();
        assert_eq!(json, "\"critical\"");
        let json = serde_json::to_string(&RiskLevel::Low).unwrap();
        assert_eq!(json, "\"low\"");
    }
}
