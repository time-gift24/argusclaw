# mcp-tool-naming

## ADDED Requirements

### Requirement: Agent tool_names SHALL support MCP tool references
The system SHALL allow agent configuration's `tool_names` field to reference MCP tools using the `mcp_{server}_{tool}` naming convention.

#### Scenario: Agent uses MCP tool
- **WHEN** agent has `tool_names: ["shell", "mcp_filesystem_read", "mcp_github_create_issue"]`
- **THEN** the agent SHALL have access to native tool "shell" and MCP tools from servers "filesystem" and "github"

### Requirement: ToolManager SHALL resolve MCP tool names to McpTool instances
The system SHALL resolve `mcp_*` prefixed tool names to MCP tools from the corresponding server, not native tools.

### Requirement: Listing tools SHALL include MCP tools
The `list_tools` command SHALL return both native tools and MCP tools with their server origin indicated.
