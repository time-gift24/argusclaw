import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const editAgentPageSource = readFileSync(
  new URL("../app/settings/agents/edit/page.tsx", import.meta.url),
  "utf8",
);

test("static agent settings edit page reads the agent id from search params", () => {
  assert.match(editAgentPageSource, /function EditAgentContent\(\)/);
  assert.match(editAgentPageSource, /const searchParams = useSearchParams\(\)/);
  assert.match(editAgentPageSource, /const id = searchParams\.get\("id"\)/);
  assert.match(editAgentPageSource, /return id \? parseInt\(id, 10\) : undefined/);
  assert.match(editAgentPageSource, /<AgentEditor agentId=\{agentId\} \/>/);
});
