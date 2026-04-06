//! MCP runtime primitives for server supervision and tool execution.

pub mod error;
pub mod runtime;
pub mod supervisor;
pub mod tool_adapter;

pub use argus_protocol::{
    AgentMcpBinding, AgentMcpServerBinding, AgentMcpToolBinding, McpDiscoveredToolRecord,
    McpServerRecord, McpServerStatus, McpToolResolver, McpTransportConfig, McpTransportKind,
    McpUnavailableServerSummary, ResolvedMcpTools, ThreadNoticeLevel,
};

pub use error::McpRuntimeError;
pub use runtime::{
    McpConnectionTestResult, McpConnector, McpRepository, McpRuntime, McpRuntimeConfig,
    McpRuntimeHandle, McpRuntimeSnapshot, McpServerRuntimeSnapshot, McpSession, RmcpConnector,
};
pub use tool_adapter::{McpToolAdapter, McpToolExecutor};
