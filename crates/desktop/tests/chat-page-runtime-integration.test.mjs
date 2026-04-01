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
  assert.match(
    chatScreenSource,
    /TabsContent[\s\S]*value="chat"[\s\S]*className="m-0 flex min-h-0 flex-1 overflow-hidden"/,
  );
  assert.match(chatScreenSource, /TabsContent[\s\S]*value="threads"/);
  assert.match(chatScreenSource, /ThreadMonitorScreen/);
  assert.match(threadMonitorScreenSource, /冷却中/);
  assert.match(threadMonitorScreenSource, /已驱逐/);
  assert.match(threadMonitorScreenSource, /kindFilter|全部类型|Thread Monitor|监控优先/);
  assert.match(threadMonitorScreenSource, /权威|authoritative/i);
  assert.match(chatScreenSource, /<Thread \/>/);
  assert.match(threadSource, /className="aui-root aui-thread-root @container relative flex h-full min-h-0 w-full flex-1 flex-col bg-background overflow-hidden"/);
  assert.match(threadSource, /ChatStatusBanner/);
  assert.match(threadSource, /jobStatuses|JobStatus/);
  assert.match(threadSource, /whitespace-pre-wrap break-words[\s\S]*job\.message/);
  assert.match(threadSource, /Reasoning:/);
  assert.match(threadSource, /pendingAssistant\.retry/);
  assert.match(threadSource, /正在重试请求/);
  assert.match(pageSource, /<ChatScreen \/>/);
});
