import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const agentEditorSource = readFileSync(
  new URL("../components/settings/agent-editor.tsx", import.meta.url),
  "utf8",
);
const bindingCardPath = new URL(
  "../components/settings/agent-mcp-binding-card.tsx",
  import.meta.url,
);

test("agent editor loads, configures, and saves MCP bindings alongside built-in tools", () => {
  assert.equal(existsSync(bindingCardPath), true);
  assert.match(agentEditorSource, /mcp,/);
  assert.match(
    agentEditorSource,
    /const \[mcpServerList, setMcpServerList\] = React\.useState<McpServerRecord\[]>\(\[\]\)/,
  );
  assert.match(
    agentEditorSource,
    /const \[mcpBindings, setMcpBindings\] = React\.useState<AgentMcpBinding\[]>\(\[\]\)/,
  );
  assert.match(agentEditorSource, /const \[mcpToolsByServerId, setMcpToolsByServerId\] = React\.useState/);
  assert.match(agentEditorSource, /const mcpEnabledCount = React\.useMemo/);
  assert.match(agentEditorSource, /const loadMcpTools = React\.useCallback/);
  assert.match(agentEditorSource, /await mcp\.listServers\(\)/);
  assert.match(agentEditorSource, /await mcp\.listAgentBindings\(agentId\)/);
  assert.match(agentEditorSource, /await mcp\.setAgentBindings\(targetId, mcpBindings\)/);
  assert.match(agentEditorSource, /await mcp\.listServerTools\(serverId\)/);
  assert.match(agentEditorSource, /<AgentMcpBindingCard/);
  assert.match(agentEditorSource, /MCP Servers/);
  assert.match(agentEditorSource, /MCP 配置/);
});
