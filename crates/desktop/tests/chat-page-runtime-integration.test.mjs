import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const chatRuntimeSource = readFileSync(new URL("../lib/chat-runtime.ts", import.meta.url), "utf8");
const chatScreenSource = readFileSync(new URL("../components/chat/chat-screen.tsx", import.meta.url), "utf8");
const threadMonitorScreenSource = readFileSync(
  new URL("../components/thread-monitor/thread-monitor-screen.tsx", import.meta.url),
  "utf8",
);
const pageSource = readFileSync(new URL("../app/page.tsx", import.meta.url), "utf8");
const threadSource = readFileSync(new URL("../components/assistant-ui/thread.tsx", import.meta.url), "utf8");

test("chat screen wires assistant-ui runtime into the thread UI", () => {
  assert.match(chatRuntimeSource, /useExternalStoreRuntime/);
  assert.match(chatRuntimeSource, /pendingAssistant/);
  assert.match(chatRuntimeSource, /type:\s*"reasoning"/);
  assert.match(chatRuntimeSource, /msg\.reasoning_content/);
  assert.match(chatRuntimeSource, /tool_calls|tool-call|toolCallId/);
  assert.match(chatRuntimeSource, /toolCallLocations/);
  assert.match(chatRuntimeSource, /result:\s*parseMessageContent/);
  assert.match(chatRuntimeSource, /createEmptyAssistantMetadata/);
  assert.match(chatRuntimeSource, /onNew:/);
  assert.match(chatScreenSource, /AssistantRuntimeProvider/);
  assert.match(chatScreenSource, /useChatRuntime\(\)/);
  assert.match(chatScreenSource, /TabsTrigger value="chat"/);
  assert.match(chatScreenSource, /TabsTrigger value="threads"/);
  assert.match(chatScreenSource, /TabsContent value="threads"/);
  assert.match(chatScreenSource, /ThreadMonitorScreen/);
  assert.match(threadMonitorScreenSource, /冷却中/);
  assert.match(threadMonitorScreenSource, /已驱逐/);
  assert.match(chatScreenSource, /<Thread \/>/);
  assert.match(threadSource, /ChatStatusBanner/);
  assert.match(threadSource, /jobStatuses|JobStatus/);
  assert.match(threadSource, /line-clamp-6[\s\S]*job\.message/);
  assert.match(threadSource, /Reasoning:/);
  assert.match(pageSource, /<ChatScreen \/>/);
});
