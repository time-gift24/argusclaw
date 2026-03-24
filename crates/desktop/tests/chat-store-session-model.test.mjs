import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const storeSource = readFileSync(new URL("../lib/chat-store.ts", import.meta.url), "utf8");

test("chat store keeps sessions keyed by template and provider preference", () => {
  assert.match(storeSource, /errorMessage:\s*string \| null/);
  assert.match(storeSource, /activeSessionKey:\s*string \| null/);
  assert.match(storeSource, /sessionsByKey:\s*Record<string,\s*ChatSessionState>/);
  assert.match(storeSource, /selectedProviderPreferenceId:\s*number \| null/);
  assert.match(storeSource, /refreshSnapshot:\s*\([\s\S]*sessionKey:\s*string/);
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
    /pendingAssistant:\s*\{\s*content:\s*string;\s*reasoning:\s*string;\s*toolCalls:\s*PendingToolCall\[\];\s*plan:\s*PlanItem\[\]\s*\|\s*null\s*\}\s*\|\s*null/,
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

test("failed update_plan completions clear any optimistic pending plan", () => {
  assert.match(
    storeSource,
    /case "tool_started":[\s\S]*?payload\.tool_name === "update_plan"[\s\S]*?plan:\s*args\.plan/,
  );
  assert.match(
    storeSource,
    /case "tool_completed":[\s\S]*?payload\.tool_name === "update_plan"[\s\S]*?plan:\s*payload\.is_error\s*\?\s*null\s*:/,
  );
});

test("turn_failed refresh preserves the frontend error state", () => {
  assert.match(
    storeSource,
    /refreshSnapshot:\s*\([\s\S]*sessionKey:\s*string,[\s\S]*options\?\s*:\s*\{\s*preserveError\?\s*:\s*boolean\s*\}/,
  );
  assert.match(
    storeSource,
    /case "turn_failed":[\s\S]*?refreshSnapshot\(sessionKey,\s*\{\s*preserveError:\s*true\s*\}\)/,
  );
  assert.match(
    storeSource,
    /errorMessage:\s*options\?\.preserveError\s*\?\s*state\.errorMessage\s*:\s*null/,
  );
  assert.match(
    storeSource,
    /status:\s*options\?\.preserveError\s*\?\s*"error"\s*:\s*"idle"/,
  );
});

test("turn_failed clears any pending assistant state before snapshot refresh", () => {
  assert.match(
    storeSource,
    /case "turn_failed":[\s\S]*?status:\s*"error"[\s\S]*?pendingAssistant:\s*null[\s\S]*?refreshSnapshot\(sessionKey,\s*\{\s*preserveError:\s*true\s*\}\)/,
  );
});
