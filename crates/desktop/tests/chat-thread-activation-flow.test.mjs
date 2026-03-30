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

test("switchToThread resets per-thread UI state instead of reusing the previous thread state", () => {
  const switchBranch = storeSource.match(
    /async switchToThread\(sessionId: string, threadId: string\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(switchBranch?.groups?.branch, "switchToThread branch should exist");
  assert.match(
    switchBranch.groups.branch,
    /jobStatuses:\s*\{\}/,
    "switching threads should clear ephemeral job statuses from the previously viewed thread",
  );
  assert.match(
    switchBranch.groups.branch,
    /contextWindow:\s*null/,
    "switching threads should force a fresh context window fetch for the activated thread",
  );
});

test("existing thread activation reuses a stable session-based forwarder key", () => {
  assert.match(commandSource, /start_forwarder\(\s*session_id\.to_string\(\)/);
  assert.match(subscriptionSource, /subscriptions: HashMap<String, CancellationToken>/);
});

test("desktop commands expose persistent session and thread rename entry points", () => {
  assert.match(commandSource, /pub async fn rename_session/);
  assert.match(commandSource, /pub async fn rename_thread/);
});
