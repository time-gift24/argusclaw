# mcp-client

## ADDED Requirements

### Requirement: MCP client SHALL implement NamedTool interface
The system SHALL wrap MCP server tools as `NamedTool` implementations, allowing them to be registered in `ToolManager` and used by agents.

### Requirement: Tool names SHALL follow mcp_{server}_{tool} convention
MCP tools SHALL be named using the format `mcp_{server_name}_{original_tool_name}` to avoid namespace collisions with native tools.

#### Scenario: Tool name construction
- **WHEN** MCP server named "filesystem" exposes a tool named "read"
- **THEN** the tool's full name SHALL be "mcp_filesystem_read"

### Requirement: McpToolError SHALL provide rich error context
When an MCP tool call fails, the error SHALL include the server name, tool name, and human-readable context.

### Requirement: MCP client pool SHALL manage server lifecycle
The system SHALL maintain a pool of MCP client runtimes, one per enabled MCP server, handling connection lifecycle (connect, reconnect on failure).

### Requirement: Tool definitions SHALL be discovered dynamically
The system SHALL query the MCP server for its available tools and expose them as `ToolDefinition` structs for LLM consumption.

### Requirement: MCP tools SHALL default to RiskLevel::Medium
MCP tools from external servers SHALL default to `RiskLevel::Medium` risk level unless the tool definition includes explicit risk annotation.

### Requirement: Version incompatibility SHALL be detected and reported
If the MCP client and server have incompatible protocol versions, the connection SHALL fail with a clear error message.
