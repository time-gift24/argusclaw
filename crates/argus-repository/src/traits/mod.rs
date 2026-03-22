//! Repository traits for data access abstraction.

mod agent;
mod job;
mod mcp_server;
mod thread;
mod workflow;

pub use agent::AgentRepository;
pub use job::JobRepository;
pub use mcp_server::McpServerRepository;
pub use thread::ThreadRepository;
pub use workflow::WorkflowRepository;

// Re-export LlmProviderRepository from argus_protocol
pub use argus_protocol::llm::LlmProviderRepository;
