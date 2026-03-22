# mcp-server-config

## ADDED Requirements

### Requirement: MCP server can be stored with transport configuration
The system SHALL support storing MCP server configurations including name, display name, transport type (Stdio or SSE), command (for Stdio), URL (for SSE), and optional auth token.

### Requirement: Auth token SHALL be encrypted at rest
The system SHALL encrypt the auth token using AES-256-GCM before storing in the database, similar to how LLM provider API keys are handled.

### Requirement: MCP server connection SHALL be testable
The system SHALL provide a way to test MCP server connectivity that returns success/failure with descriptive error messages.

#### Scenario: Test stdio transport MCP server
- **WHEN** user clicks "Test Connection" on a Stdio transport MCP server with valid command
- **THEN** system spawns the MCP server process, initializes the connection, discovers tools, and returns success with tool list count

#### Scenario: Test SSE transport MCP server
- **WHEN** user clicks "Test Connection" on an SSE transport MCP server with valid URL
- **THEN** system establishes SSE connection, initializes the session, and returns success with tool list count

#### Scenario: Connection failure during init does not block other servers
- **WHEN** register_mcp_tools() connects to multiple MCP servers and one fails
- **THEN** system logs the error for the failed server and continues registering remaining servers

#### Scenario: Connection failure shows descriptive error
- **WHEN** user tests connection to unreachable MCP server
- **THEN** system returns failure with error context including reason (timeout, protocol mismatch, auth failure)

### Requirement: MCP server configuration SHALL be unique by name
The system SHALL enforce unique names for MCP servers to prevent tool name collisions.

### Requirement: MCP server can be enabled or disabled
The system SHALL allow enabling and disabling MCP servers without deleting their configuration.
