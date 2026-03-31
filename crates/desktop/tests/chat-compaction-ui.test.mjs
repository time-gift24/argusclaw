import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const typeSource = readFileSync(new URL("../lib/types/chat.ts", import.meta.url), "utf8");
const runtimeSource = readFileSync(new URL("../lib/chat-runtime.ts", import.meta.url), "utf8");
const threadSource = readFileSync(new URL("../components/assistant-ui/thread.tsx", import.meta.url), "utf8");
const bannerSource = readFileSync(new URL("../components/chat/chat-status-banner.tsx", import.meta.url), "utf8");

test("desktop chat types expose compaction message metadata and lifecycle events", () => {
  assert.match(typeSource, /export interface ChatMessageMetadataPayload/);
  assert.match(typeSource, /metadata\?: ChatMessageMetadataPayload \| null/);
  assert.match(typeSource, /\| \{ type: "compaction_started" \}/);
  assert.match(typeSource, /\| \{ type: "compaction_finished" \}/);
  assert.match(typeSource, /\| \{ type: "compaction_failed"; error: string \}/);
});

test("chat runtime groups folded compaction messages into a synthetic context block", () => {
  assert.match(runtimeSource, /compactionGroup/i);
  assert.match(runtimeSource, /collapsed_by_default/);
  assert.match(runtimeSource, /CompactionSummary|compaction_summary/);
});

test("thread UI disables composing during compaction and renders a folded compaction block", () => {
  assert.match(threadSource, /status === "compacting"/);
  assert.match(threadSource, /已压缩上下文/);
  assert.match(threadSource, /CompactionGroup|compactionGroup/);
  assert.match(bannerSource, /上下文压缩中/);
});
