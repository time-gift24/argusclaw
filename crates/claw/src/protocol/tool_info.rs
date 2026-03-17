//! Tool information for frontend consumption.

use serde::{Deserialize, Serialize};

use super::RiskLevel;

/// Information about a tool for display in the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Unique name of the tool.
    pub name: String,
    /// Human-readable description of the tool.
    pub description: String,
    /// Risk level of the tool for approval gating.
    pub risk_level: RiskLevel,
}
