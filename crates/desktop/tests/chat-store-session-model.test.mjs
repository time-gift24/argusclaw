import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const storeSource = readFileSync(new URL("../lib/chat-store.ts", import.meta.url), "utf8");

test("chat store keeps sessions keyed by template and provider preference", () => {
  assert.match(storeSource, /activeSessionKey:\s*string \| null/);
  assert.match(storeSource, /sessionsByKey:\s*Record<string,\s*ChatSessionState>/);
  assert.match(storeSource, /selectedProviderPreferenceId:\s*string \| null/);
  assert.match(storeSource, /refreshSnapshot:\s*\(sessionKey: string\)/);
  assert.match(storeSource, /listen.*"thread:event"/);
  assert.match(storeSource, /thread_id|threadId/);
  assert.match(storeSource, /TurnCompleted/);
});
