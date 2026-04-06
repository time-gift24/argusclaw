import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const typeSource = readFileSync(
  new URL("../lib/types/chat.ts", import.meta.url),
  "utf8",
);
const storeSource = readFileSync(
  new URL("../lib/chat-store.ts", import.meta.url),
  "utf8",
);
const runtimeSource = readFileSync(
  new URL("../lib/chat-runtime.ts", import.meta.url),
  "utf8",
);
const threadSource = readFileSync(
  new URL("../components/assistant-ui/thread.tsx", import.meta.url),
  "utf8",
);

test("chat message payload supports local optimistic delivery metadata", () => {
  assert.match(typeSource, /local_delivery_status\?:\s*"failed"/);
  assert.match(typeSource, /local_client_id\?:\s*string/);
});

test("sendMessage appends an optimistic local user message before backend send", () => {
  const sendMessageBranch = storeSource.match(
    /async sendMessage\(content: string\) \{(?<branch>[\s\S]*?)\n  \},/,
  );

  assert.ok(sendMessageBranch?.groups?.branch, "sendMessage branch should exist");
  assert.match(sendMessageBranch.groups.branch, /const localClientId =/);
  assert.match(
    sendMessageBranch.groups.branch,
    /const optimisticUserMessage:\s*ChatMessagePayload = \{[\s\S]*role:\s*"user"[\s\S]*local_client_id:\s*localClientId/,
  );
  assert.match(
    sendMessageBranch.groups.branch,
    /messages:\s*\[\.\.\.session\.messages,\s*optimisticUserMessage\]/,
  );

  const optimisticSetIndex = sendMessageBranch.groups.branch.indexOf(
    "messages: [...session.messages, optimisticUserMessage]",
  );
  const backendSendIndex = sendMessageBranch.groups.branch.indexOf(
    "await chat.sendMessage(",
  );
  assert.ok(optimisticSetIndex >= 0, "optimistic append should exist");
  assert.ok(backendSendIndex >= 0, "backend send should exist");
  assert.ok(
    optimisticSetIndex < backendSendIndex,
    "optimistic append should happen before backend send",
  );
});

test("sendMessage failure keeps local message and marks it as failed", () => {
  const sendMessageBranch = storeSource.match(
    /async sendMessage\(content: string\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(sendMessageBranch?.groups?.branch, "sendMessage branch should exist");
  assert.match(
    sendMessageBranch.groups.branch,
    /local_delivery_status:[\s\S]*msg\.local_client_id === localClientId[\s\S]*"failed"[\s\S]*msg\.local_delivery_status/,
  );
  assert.match(
    sendMessageBranch.groups.branch,
    /status:\s*"error"[\s\S]*pendingAssistant:\s*null/,
  );
});

test("chat runtime propagates optimistic delivery metadata into assistant-ui message custom metadata", () => {
  assert.match(runtimeSource, /localDeliveryStatus:\s*msg\.local_delivery_status \?\? null/);
  assert.match(runtimeSource, /localClientId:\s*msg\.local_client_id \?\? null/);
});

test("user message bubble renders an inline failed-delivery indicator", () => {
  assert.match(threadSource, /const localDeliveryStatus = useAuiState/);
  assert.match(threadSource, /localDeliveryStatus === "failed"/);
  assert.match(threadSource, /发送失败/);
});
