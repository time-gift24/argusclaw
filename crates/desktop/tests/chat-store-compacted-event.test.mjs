import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const storeSource = readFileSync(new URL("../lib/chat-store.ts", import.meta.url), "utf8");

test("chat store applies compacted events without disturbing streaming state", () => {
  const compactedBranch = storeSource.match(
    /case "compacted":(?<branch>[\s\S]*?)break;/,
  );

  assert.ok(compactedBranch?.groups?.branch, "compacted branch should exist");
  assert.match(
    compactedBranch.groups.branch,
    /tokenCount:\s*payload\.new_token_count/,
    "compacted should eagerly sync token count",
  );
  assert.doesNotMatch(
    compactedBranch.groups.branch,
    /status:/,
    "compacted should not change session status",
  );
  assert.doesNotMatch(
    compactedBranch.groups.branch,
    /pendingAssistant:\s*null/,
    "compacted should not clear pending assistant state",
  );
  assert.doesNotMatch(
    compactedBranch.groups.branch,
    /refreshSnapshot\(sessionKey\)/,
    "compacted should not force an extra snapshot refresh",
  );
});
