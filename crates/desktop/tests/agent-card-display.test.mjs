import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const agentCardPath = new URL(
  "../components/settings/agent-card.tsx",
  import.meta.url,
);
const agentsPagePath = new URL(
  "../app/settings/agents/page.tsx",
  import.meta.url,
);

test("agent list cards receive provider metadata and render compact metrics", () => {
  const agentCardSource = readFileSync(agentCardPath, "utf8");
  const agentsPageSource = readFileSync(agentsPagePath, "utf8");

  assert.match(agentsPageSource, /providers=\{providerList\}/);
  assert.match(agentCardSource, /providers:\s*LlmProviderSummary\[\]/);
  assert.match(
    agentCardSource,
    /providers\.find\(\(provider\)\s*=>\s*provider\.id === agent\.provider_id\)\?\.display_name/,
  );
  assert.match(
    agentCardSource,
    /const toolNames = \[\.\.\.new Set\(agent\.tool_names\.filter\(Boolean\)\)\]/,
  );
  assert.match(
    agentCardSource,
    /hidden md:flex items-center gap-6 px-6 border-x border-muted\/30 h-8/,
  );
  assert.match(agentCardSource, />提供者<\/span>/);
  assert.match(agentCardSource, />工具<\/span>/);
  assert.match(agentCardSource, />温度<\/span>/);
  assert.match(agentCardSource, /toolNames\.length > 0 \? `\$\{toolNames\.length\} 个` : "无"/);
  assert.match(agentCardSource, /const DEFAULT_TEMPERATURE = 0\.7/);
  assert.match(agentCardSource, /function formatTemperature\(temperature\?: number\)/);
  assert.match(agentCardSource, /formatTemperature\(agent\.temperature\)/);
  assert.match(agentCardSource, /<span className="sr-only">编辑<\/span>/);
  assert.match(agentCardSource, /<span className="sr-only">删除<\/span>/);
  assert.doesNotMatch(agentCardSource, /<DetailRow label="提供者">/);
});
