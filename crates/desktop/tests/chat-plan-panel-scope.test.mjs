import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const threadSource = readFileSync(
  new URL("../components/assistant-ui/thread.tsx", import.meta.url),
  "utf8",
);

test("plan panel gating uses message scope instead of part scope", () => {
  assert.doesNotMatch(threadSource, /s\.part\.status\.type === "running"/);
  assert.match(threadSource, /s\.message\.status\?\.type === "running"/);
});
