//! Repository traits for data access abstraction.

mod agent;
mod job;
mod thread;
mod workflow;

pub use agent::AgentRepository;
pub use job::JobRepository;
pub use thread::ThreadRepository;
pub use workflow::WorkflowRepository;

// Re-export LlmProviderRepository from argus_protocol
pub use argus_protocol::llm::LlmProviderRepository;
