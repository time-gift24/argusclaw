import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const editAgentPageSource = readFileSync(
  new URL("../app/settings/agents/[id]/page.tsx", import.meta.url),
  "utf8",
);

test("dynamic agent settings page awaits Next 16 route params", () => {
  assert.match(editAgentPageSource, /async function EditAgentPage/);
  assert.match(editAgentPageSource, /params:\s*Promise<\{\s*id:\s*string\s*\}>/);
  assert.match(editAgentPageSource, /const\s*\{\s*id\s*\}\s*=\s*await params/);
});
