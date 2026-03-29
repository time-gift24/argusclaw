//! Persistence types (records, IDs).

mod agent;
mod job;
mod thread;
mod workflow;

pub use agent::{AgentId, AgentRecord};
pub use job::{JobRecord, JobResult, JobType};
pub use thread::{MessageId, MessageRecord, SessionRecord, ThreadRecord};
pub use workflow::{
    JobId, WorkflowExecutionHeader, WorkflowExecutionNodeRecord, WorkflowExecutionRecord,
    WorkflowId, WorkflowProgressRecord, WorkflowRecord, WorkflowStatus, WorkflowTemplateId,
    WorkflowTemplateNodeRecord, WorkflowTemplateRecord,
};
