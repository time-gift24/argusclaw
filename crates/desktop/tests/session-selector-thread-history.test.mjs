import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const sessionSelectorSource = readFileSync(
  new URL("../components/assistant-ui/session-selector.tsx", import.meta.url),
  "utf8",
);
const storeSource = readFileSync(
  new URL("../lib/chat-store.ts", import.meta.url),
  "utf8",
);

test("chat store exposes per-session thread loading and switching", () => {
  assert.match(storeSource, /threadListBySessionId:\s*Record<string,\s*ThreadSummary\[\]>/);
  assert.match(
    storeSource,
    /threadListLoadingBySessionId:\s*Record<string,\s*boolean>/,
  );
  assert.match(storeSource, /loadThreads:\s*\(sessionId:\s*string\)\s*=>\s*Promise<void>/);
  assert.match(
    storeSource,
    /switchToThread:\s*\(sessionId:\s*string,\s*threadId:\s*string\)\s*=>\s*Promise<void>/,
  );
});

test("session selector renders thread items and switches to the clicked thread", () => {
  assert.match(sessionSelectorSource, /const loadThreads = useChatStore/);
  assert.match(sessionSelectorSource, /const switchToThread = useChatStore/);
  assert.match(sessionSelectorSource, /threadListBySessionId/);
  assert.match(sessionSelectorSource, /loadThreads\(sessionId\)/);
  assert.match(sessionSelectorSource, /handleSwitchThread\(session\.id,\s*thread\.thread_id\)/);
  assert.match(sessionSelectorSource, /thread\.turn_count/);
});
