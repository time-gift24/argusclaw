import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const storeSource = readFileSync(new URL("../lib/chat-store.ts", import.meta.url), "utf8");
const tauriSource = readFileSync(new URL("../lib/tauri.ts", import.meta.url), "utf8");
const commandSource = readFileSync(new URL("../src-tauri/src/commands.rs", import.meta.url), "utf8");
const subscriptionSource = readFileSync(new URL("../src-tauri/src/subscription.rs", import.meta.url), "utf8");

test("switchToThread activates an existing thread before fetching its snapshot", () => {
  assert.match(tauriSource, /activateExistingThread:/);
  assert.match(commandSource, /pub async fn activate_existing_thread/);
  assert.match(storeSource, /await chat\.activateExistingThread\(sessionId,\s*threadId\)/);
  assert.match(storeSource, /await chat\.getThreadSnapshot\(sessionId,\s*threadId\)/);
});

test("switchToThread keeps session-scoped job state while resetting transient selection state", () => {
  const switchBranch = storeSource.match(
    /async switchToThread\(sessionId: string, threadId: string\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(switchBranch?.groups?.branch, "switchToThread branch should exist");
  assert.match(
    switchBranch.groups.branch,
    /jobStatuses:\s*existingSession\?\.jobStatuses\s*\?\?\s*\{\}/,
    "switching threads should preserve session-scoped job statuses across thread activation",
  );
  assert.match(
    switchBranch.groups.branch,
    /jobDetails:\s*existingSession\?\.jobDetails\s*\?\?\s*\{\}/,
    "switching threads should preserve session-scoped job details across thread activation",
  );
  assert.match(
    switchBranch.groups.branch,
    /contextWindow:\s*null/,
    "switching threads should force a fresh context window fetch for the activated thread",
  );
});

test("existing thread activation keeps per-session forwarders for multiple threads alive", () => {
  assert.match(commandSource, /start_forwarder\(\s*session_id\.to_string\(\)/);
  assert.match(
    subscriptionSource,
    /subscriptions: HashMap<String,\s*HashMap<String,\s*CancellationToken>>/,
  );
});

test("desktop commands expose persistent session and thread rename entry points", () => {
  assert.match(commandSource, /pub async fn rename_session/);
  assert.match(commandSource, /pub async fn rename_thread/);
});
