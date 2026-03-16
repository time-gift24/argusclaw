use serde::{Deserialize, Serialize};

use crate::{AgentId, LlmProviderId};

/// A handle to a runtime agent created from a template.
///
/// This type is returned by `AppContext::create_runtime_agent_from_template`
/// and captures the relationship between a template and its runtime instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAgentHandle {
    /// The unique ID of the runtime agent instance.
    pub runtime_agent_id: AgentId,
    /// The template ID this runtime agent was created from.
    pub template_id: AgentId,
    /// The effective provider ID bound to this runtime agent.
    pub effective_provider_id: LlmProviderId,
}
