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
    /const dispatchCapable = agent\.subagent_names\.length > 0/,
  );
  assert.match(
    agentCardSource,
    /hidden h-8 items-center gap-4 border-x border-muted\/30 px-4 md:flex/,
  );
  assert.match(agentCardSource, />提供者<\/span>/);
  assert.match(agentCardSource, />子代理<\/span>/);
  assert.match(agentCardSource, />温度<\/span>/);
  assert.match(agentCardSource, /dispatchCapable \? "可调度" : "单体"/);
  assert.match(agentCardSource, /agent\.subagent_names\.length > 0 \? `\$\{agent\.subagent_names\.length\} 个` : "无"/);
  assert.match(agentCardSource, /const DEFAULT_TEMPERATURE = 0\.7/);
  assert.match(agentCardSource, /function formatTemperature\(temperature\?: number\)/);
  assert.match(agentCardSource, /formatTemperature\(agent\.temperature\)/);
  assert.match(agentCardSource, /<span className="sr-only">编辑<\/span>/);
  assert.match(agentCardSource, /<span className="sr-only">删除<\/span>/);
  assert.doesNotMatch(agentCardSource, /<DetailRow label="提供者">/);
});
