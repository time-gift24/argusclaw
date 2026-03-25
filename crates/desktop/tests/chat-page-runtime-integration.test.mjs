import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const chatRuntimeSource = readFileSync(new URL("../lib/chat-runtime.ts", import.meta.url), "utf8");
const chatScreenSource = readFileSync(new URL("../components/chat/chat-screen.tsx", import.meta.url), "utf8");
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
  assert.match(chatScreenSource, /<Thread \/>/);
  assert.match(threadSource, /ChatStatusBanner/);
  assert.match(threadSource, /jobStatuses|JobStatus/);
  assert.match(threadSource, /Reasoning:/);
  assert.match(pageSource, /<ChatScreen \/>/);
});
