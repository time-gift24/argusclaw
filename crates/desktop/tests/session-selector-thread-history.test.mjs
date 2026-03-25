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
  assert.match(sessionSelectorSource, /loadThreads\(/);
  assert.match(sessionSelectorSource, /handleSwitchThread\([^,]+,\s*thread\.thread_id\)/);
  assert.match(sessionSelectorSource, /thread\.turn_count/);
});

test("session actions are split into new-session and history entry points", () => {
  assert.match(sessionSelectorSource, /export function NewSessionButton/);
  assert.match(sessionSelectorSource, /export function SessionHistoryButton/);
  assert.doesNotMatch(sessionSelectorSource, /export function SessionSelector/);
  assert.doesNotMatch(sessionSelectorSource, /\/\* New Session Button \*\//);
  assert.doesNotMatch(sessionSelectorSource, /基于当前智能体创建/);
});

test("history dialog uses a left-right session and thread layout", () => {
  assert.match(sessionSelectorSource, /selectedSessionId/);
  assert.match(sessionSelectorSource, /loadThreads\(selectedSessionId\)/);
  assert.match(sessionSelectorSource, /function displaySessionName/);
  assert.match(sessionSelectorSource, /function displayThreadName/);
  assert.match(sessionSelectorSource, /grid-cols-\[minmax\(0,1fr\)_minmax\(0,1\.15fr\)\]/);
});

test("history dialog exposes right-click rename affordances", () => {
  assert.match(sessionSelectorSource, /onContextMenu/);
  assert.match(sessionSelectorSource, /重命名 Session/);
  assert.match(sessionSelectorSource, /重命名 Thread/);
  assert.match(sessionSelectorSource, /sessions\.renameSession/);
  assert.match(sessionSelectorSource, /sessions\.renameThread/);
});

test("history dialog keeps timestamp rendering hydration-safe", () => {
  assert.match(sessionSelectorSource, /const \[hasMounted, setHasMounted\] = React\.useState\(false\)/);
  assert.match(sessionSelectorSource, /React\.useEffect\(\(\) => \{\s*setHasMounted\(true\);/);
});
