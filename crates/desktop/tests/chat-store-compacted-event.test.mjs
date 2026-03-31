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

test("chat store tracks compact-agent lifecycle with a dedicated compacting status", () => {
  assert.match(
    storeSource,
    /status:\s*"idle"\s*\|\s*"running"\s*\|\s*"compacting"\s*\|\s*"error"/,
    "chat session status union should include compacting",
  );

  const startedBranch = storeSource.match(
    /case "compaction_started":(?<branch>[\s\S]*?)break;/,
  );
  assert.ok(startedBranch?.groups?.branch, "compaction_started branch should exist");
  assert.match(
    startedBranch.groups.branch,
    /status:\s*"compacting"/,
    "compaction_started should switch the session into compacting state",
  );
  assert.doesNotMatch(
    startedBranch.groups.branch,
    /tokenCount:/,
    "compaction_started should not mutate authoritative token counts",
  );

  const finishedBranch = storeSource.match(
    /case "compaction_finished":(?<branch>[\s\S]*?)break;/,
  );
  assert.ok(finishedBranch?.groups?.branch, "compaction_finished branch should exist");
  assert.match(
    finishedBranch.groups.branch,
    /status:\s*"running"/,
    "compaction_finished should hand control back to the visible turn",
  );

  const failedBranch = storeSource.match(
    /case "compaction_failed":(?<branch>[\s\S]*?)break;/,
  );
  assert.ok(failedBranch?.groups?.branch, "compaction_failed branch should exist");
  assert.match(
    failedBranch.groups.branch,
    /status:\s*"running"/,
    "compaction_failed should resume the visible turn instead of leaving the thread stuck",
  );
});
