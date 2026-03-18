//! Job domain types.
//!
//! This module re-exports types from `argus_repository` for backward compatibility.

// Transitional re-exports
#![allow(unused_imports)]

// Re-export types from argus_repository::types
pub use argus_repository::types::{JobRecord, JobType};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_type_roundtrip() {
        for (variant, expected) in [
            (JobType::Standalone, "standalone"),
            (JobType::Workflow, "workflow"),
            (JobType::Cron, "cron"),
        ] {
            assert_eq!(variant.as_str(), expected);
            assert_eq!(JobType::parse_str(expected).unwrap(), variant);
        }
    }

    #[test]
    fn job_type_invalid() {
        assert!(JobType::parse_str("invalid").is_err());
        assert!(JobType::parse_str("STANDALONE").is_err());
        assert!(JobType::parse_str("").is_err());
    }

    #[test]
    fn job_type_display() {
        assert_eq!(JobType::Standalone.to_string(), "standalone");
        assert_eq!(JobType::Workflow.to_string(), "workflow");
        assert_eq!(JobType::Cron.to_string(), "cron");
    }
}
