import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const tauriBindingsSource = readFileSync(
  new URL("../lib/tauri.ts", import.meta.url),
  "utf8",
);
const tauriCommandsSource = readFileSync(
  new URL("../src-tauri/src/lib.rs", import.meta.url),
  "utf8",
);

test("desktop tauri bindings expose MCP server CRUD, testing, and agent binding APIs", () => {
  assert.match(tauriBindingsSource, /export type McpServerStatus =/);
  assert.match(tauriBindingsSource, /export type McpTransportConfig =/);
  assert.match(tauriBindingsSource, /export interface McpServerRecord/);
  assert.match(tauriBindingsSource, /export interface McpConnectionTestResult/);
  assert.match(tauriBindingsSource, /export interface AgentMcpBinding/);
  assert.match(tauriBindingsSource, /export const mcp = \{/);
  assert.match(
    tauriBindingsSource,
    /listServers:\s*\(\)\s*=>\s*invoke<McpServerRecord\[]>\("list_mcp_servers"\)/,
  );
  assert.match(
    tauriBindingsSource,
    /testInput:\s*\(record: McpServerRecord\)\s*=>\s*invoke<McpConnectionTestResult>\("test_mcp_server_input",\s*\{ record \}\)/,
  );
  assert.match(
    tauriBindingsSource,
    /testConnection:\s*\(id: number\)\s*=>\s*invoke<McpConnectionTestResult>\("test_mcp_server_connection",\s*\{ id \}\)/,
  );
  assert.match(
    tauriBindingsSource,
    /listAgentBindings:\s*\(agentId: number\)\s*=>\s*invoke<AgentMcpBinding\[]>\("list_agent_mcp_bindings",\s*\{ agentId \}\)/,
  );
  assert.match(
    tauriBindingsSource,
    /setAgentBindings:\s*\(agentId: number, bindings: AgentMcpBinding\[]\)\s*=>\s*invoke<void>\("set_agent_mcp_bindings",\s*\{ agentId, bindings \}\)/,
  );
});

test("tauri command registry includes MCP settings commands", () => {
  assert.match(tauriCommandsSource, /commands::list_mcp_servers/);
  assert.match(tauriCommandsSource, /commands::get_mcp_server/);
  assert.match(tauriCommandsSource, /commands::upsert_mcp_server/);
  assert.match(tauriCommandsSource, /commands::delete_mcp_server/);
  assert.match(tauriCommandsSource, /commands::test_mcp_server_input/);
  assert.match(tauriCommandsSource, /commands::test_mcp_server_connection/);
  assert.match(tauriCommandsSource, /commands::list_mcp_server_tools/);
  assert.match(tauriCommandsSource, /commands::list_agent_mcp_bindings/);
  assert.match(tauriCommandsSource, /commands::set_agent_mcp_bindings/);
});
