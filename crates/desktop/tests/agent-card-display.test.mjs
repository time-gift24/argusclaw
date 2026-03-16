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

test("agent list cards receive provider metadata and render stable parameter rows", () => {
  const agentCardSource = readFileSync(agentCardPath, "utf8");
  const agentsPageSource = readFileSync(agentsPagePath, "utf8");

  assert.match(agentsPageSource, /providers=\{providerList\}/);
  assert.match(agentCardSource, /providers:\s*LlmProviderSummary\[\]/);
  assert.match(
    agentCardSource,
    /providers\.find\(\(provider\)\s*=>\s*provider\.id === agent\.provider_id\)\?\.display_name/,
  );
  assert.match(agentCardSource, /<DetailRow label="提供者">/);
  assert.match(agentCardSource, /<DetailRow label="工具">/);
  assert.match(agentCardSource, /<DetailRow[\s\S]*<span>最大 Token<\/span>/);
  assert.match(agentCardSource, /<DetailRow label="温度">/);
  assert.match(agentCardSource, /const toolNames = agent\.tool_names\.filter\(Boolean\)/);
  assert.match(agentCardSource, /toolNames\.length > 0[\s\S]*toolNames\.map/);
  assert.match(agentCardSource, /<div className="min-w-0 text-xs">/);
  assert.match(agentCardSource, /const DEFAULT_MAX_TOKENS = 4096/);
  assert.match(agentCardSource, /const DEFAULT_TEMPERATURE = 0\.7/);
  assert.match(agentCardSource, /TooltipContent side="top">模型单次 turn 允许返回的最大 token<\/TooltipContent>/);
  assert.match(agentCardSource, /aria-label="最大 Token 说明"/);
  assert.match(agentCardSource, /function formatMaxTokens\(maxTokens\?: number\)/);
  assert.match(agentCardSource, /function formatTemperature\(temperature\?: number\)/);
  assert.match(agentCardSource, /formatMaxTokens\(agent\.max_tokens\)/);
  assert.match(agentCardSource, /formatTemperature\(agent\.temperature\)/);
  assert.match(agentCardSource, /编辑/);
  assert.match(agentCardSource, /删除/);
});
