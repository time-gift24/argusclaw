import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const storeSource = readFileSync(new URL("../lib/chat-store.ts", import.meta.url), "utf8");

test("chat store keeps sessions keyed by template and provider preference", () => {
  assert.match(storeSource, /errorMessage:\s*string \| null/);
  assert.match(storeSource, /activeSessionKey:\s*string \| null/);
  assert.match(storeSource, /sessionsByKey:\s*Record<string,\s*ChatSessionState>/);
  assert.match(storeSource, /selectedProviderPreferenceId:\s*number \| null/);
  assert.match(storeSource, /refreshSnapshot:\s*\(sessionKey: string\)/);
  assert.match(storeSource, /listen.*"thread:event"/);
  assert.match(storeSource, /thread_id|threadId/);
  assert.match(storeSource, /case "content_delta"/);
  assert.match(storeSource, /case "reasoning_delta"/);
  assert.match(storeSource, /case "turn_completed"/);
  assert.match(storeSource, /case "waiting_for_approval"/);
  assert.match(storeSource, /case "approval_resolved"/);
  assert.match(storeSource, /case "idle"/);
  assert.match(storeSource, /await get\(\)\.activateSession\(/);
  assert.match(storeSource, /chat\.createChatSession\(/);
  assert.match(storeSource, /chat\.getThreadSnapshot\(/);
  assert.match(storeSource, /catch \(error\)/);
  assert.match(storeSource, /errorMessage:/);
});

test("chat store tracks pending reasoning alongside streamed assistant text", () => {
  assert.match(
    storeSource,
    /pendingAssistant:\s*\{\s*content:\s*string;\s*reasoning:\s*string\s*\}\s*\|\s*null/,
  );
  assert.match(
    storeSource,
    /case "reasoning_delta":[\s\S]*?pendingAssistant:[\s\S]*?reasoning:\s*session\.pendingAssistant\.reasoning \+ payload\.delta/,
  );
});

test("chat store waits for idle before refreshing the persisted snapshot", () => {
  const turnCompletedBranch = storeSource.match(
    /case "turn_completed":(?<branch>[\s\S]*?)break;/,
  );
  assert.ok(turnCompletedBranch, "turn_completed branch should still exist for status handling");
  assert.doesNotMatch(
    turnCompletedBranch.groups?.branch ?? "",
    /refreshSnapshot\(sessionKey\)/,
    "turn_completed should not refresh snapshot before history is durable",
  );
  assert.match(
    storeSource,
    /case "idle":[\s\S]*?refreshSnapshot\(sessionKey\)/,
    "idle should trigger the final snapshot refresh",
  );
});
