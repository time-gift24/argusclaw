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
  assert.match(storeSource, /listen[\s\S]*"thread:event"/);
  assert.match(storeSource, /thread_id|threadId/);
  assert.match(storeSource, /case "content_delta"/);
  assert.match(storeSource, /case "reasoning_delta"/);
  assert.match(storeSource, /case "turn_completed"/);
  assert.match(storeSource, /case "job_dispatched"/);
  assert.match(storeSource, /case "job_result"/);
  assert.match(storeSource, /case "waiting_for_approval"/);
  assert.match(storeSource, /case "approval_resolved"/);
  assert.match(storeSource, /case "idle"/);
  assert.match(storeSource, /await get\(\)\.activateSession\(/);
  assert.match(storeSource, /chat\.createChatSession\(/);
  assert.match(storeSource, /chat\.getThreadSnapshot\(/);
  assert.match(storeSource, /catch \(error\)/);
  assert.match(storeSource, /errorMessage:/);
});

test("chat store guards thread-event listener registration against concurrent initialize calls", () => {
  assert.match(
    storeSource,
    /threadEventListenerInitPromise|listenerInitPromise|initializingThreadEventListener/,
    "store should track an in-flight listener registration promise",
  );
  assert.match(
    storeSource,
    /if\s*\(!get\(\)\._unlisten\)\s*\{[\s\S]*?await\s+.*threadEvent.*Promise/i,
    "initialize should await the shared listener registration instead of calling listen twice",
  );
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

test("chat store tracks ephemeral job status outside the transcript", () => {
  assert.match(
    storeSource,
    /jobStatuses:\s*Record<string,\s*JobStatusPayload>/,
    "session state should keep per-job status for temporary UI rendering",
  );
  assert.match(
    storeSource,
    /case "job_dispatched":[\s\S]*status:\s*"running"/,
    "job_dispatched should mark the job as running",
  );
  assert.match(
    storeSource,
    /case "job_result":[\s\S]*status:\s*payload\.success\s*\?\s*"completed"\s*:\s*"failed"/,
    "job_result should only update job status instead of appending transcript text",
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

test("store defers session creation until explicit new-session or first send", () => {
  const initializeBranch = storeSource.match(
    /async initialize\(\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(initializeBranch?.groups?.branch, "initialize branch should exist");
  assert.doesNotMatch(
    initializeBranch.groups.branch,
    /createChatSession|activateSession/,
    "initialize should not create or activate sessions",
  );
  assert.match(
    initializeBranch.groups.branch,
    /selectedTemplateId:\s*state\.selectedTemplateId\s*\?\?\s*templateList\[0\]\?\.id\s*\?\?\s*null/,
    "initialize should seed the selected template without creating a session",
  );

  const providerBranch = storeSource.match(
    /async selectProviderPreference\(providerId: number \| null\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(providerBranch?.groups?.branch, "selectProviderPreference branch should exist");
  assert.doesNotMatch(
    providerBranch.groups.branch,
    /activateSession\(/,
    "changing provider preference should not auto-create a session",
  );

  const modelBranch = storeSource.match(
    /async selectModelOverride\(model: string \| null\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(modelBranch?.groups?.branch, "selectModelOverride branch should exist");
  assert.doesNotMatch(
    modelBranch.groups.branch,
    /activateSession\(/,
    "changing model preference should not auto-create a session",
  );

  const activateSessionBranch = storeSource.match(
    /async activateSession\(templateId: number\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(activateSessionBranch?.groups?.branch, "activateSession branch should exist");
  assert.doesNotMatch(
    activateSessionBranch.groups.branch,
    /sessionId === state\.activeSessionKey|existingById/,
    "explicit new-session activation should always create a fresh session",
  );
});
