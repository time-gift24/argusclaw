//! MCP tool support for argus-tool.
//!
//! This module provides MCP server integration through the pmcp crate,
//! allowing MCP servers to be used as tools in the argusclaw agent system.

pub mod adapter;
pub mod client;

pub use adapter::McpTool;
pub use client::{McpClientError, McpClientRuntime};
