//! MCP client module for connecting to external MCP servers.
//!
//! This module provides:
//! - `McpClientPool`: Manages connections to multiple MCP servers
//! - `McpTool`: Wraps an MCP server tool as a `NamedTool`
//! - `ConnectionTestResult`: Result of testing MCP server connectivity

pub mod client;
pub mod mcp_error;

pub use client::{ConnectionTestResult, McpClientPool, McpTool};
pub use mcp_error::{McpClientError, Result};
