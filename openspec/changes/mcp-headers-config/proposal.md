## Why

The MCP server configuration already supports HTTP headers (as defined in `mcp-server-config` spec), and the backend fully supports it. However, the frontend form is missing the UI to actually configure headers for HTTP-type MCP servers, making it impossible for users to input authentication tokens or other required headers.

## What Changes

- Add HTTP headers configuration UI to the MCP server form dialog
- Allow users to add/remove key-value pairs for HTTP request headers
- Headers are required for many MCP servers that use Bearer token authentication

## Capabilities

### New Capabilities

- `mcp-headers-ui`: UI component for configuring HTTP headers as key-value pairs in the MCP server form

### Modified Capabilities

- `mcp-server-config`: Extend the UI requirements to explicitly include headers input field for HTTP type servers

## Impact

- **Frontend**: `mcp-server-form-dialog.tsx` needs new headers input UI
- **Types**: `McpServerPayload` already has `headers?: Record<string, string>` - no type change needed
