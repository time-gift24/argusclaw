## Context

The MCP server form dialog (`mcp-server-form-dialog.tsx`) currently shows/hides fields based on server type (Stdio vs HTTP), but it lacks a UI for configuring HTTP headers. Many HTTP-based MCP servers (like the zread example: `https://open.bigmodel.cn/api/mcp/zread/mcp` with `Authorization: Bearer ...`) require headers for authentication.

## Goals / Non-Goals

**Goals:**
- Add UI for users to add/remove HTTP headers as key-value pairs
- Only show headers input when server type is "Http"
- Maintain the existing form behavior and validation

**Non-Goals:**
- Changing the backend (already supports headers)
- Adding headers support for Stdio type (not applicable)
- Complex header manipulation (add only, simple key-value)

## Decisions

### UI Pattern: Dynamic Key-Value Input

**Decision**: Use a simple dynamic input pattern where users can add/remove header key-value pairs.

**Alternatives Considered**:
- JSON text input: Too error-prone for non-technical users
- Pre-defined templates: Too restrictive

**Implementation**:
- Show "+ Add Header" button when server type is Http
- Each header row has: Key input, Value input, Remove button
- Store headers as `Record<string, string>` in form state

## Risks / Trade-offs

- **Risk**: Users might not know what headers to add
  - **Mitigation**: Show placeholder text like "Authorization" for key, "Bearer your_token" for value
- **Risk**: Empty headers object vs undefined
  - **Mitigation**: Use `{}` as default, filter out empty entries on submit
