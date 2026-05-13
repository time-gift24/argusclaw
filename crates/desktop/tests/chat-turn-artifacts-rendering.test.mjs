import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const chatRuntimeSource = readFileSync(
  new URL("../lib/chat-runtime.ts", import.meta.url),
  "utf8",
);
const threadSource = readFileSync(
  new URL("../components/assistant-ui/thread.tsx", import.meta.url),
  "utf8",
);

test("settled assistant turns preserve ordered reasoning and tool artifacts in message metadata", () => {
  assert.match(chatRuntimeSource, /type TurnArtifacts = \{/);
  assert.match(chatRuntimeSource, /items:\s*readonly TurnArtifactItem\[\]/);
  assert.match(chatRuntimeSource, /type:\s*"reasoning"/);
  assert.match(chatRuntimeSource, /type:\s*"tool_call"/);
  assert.match(chatRuntimeSource, /turnArtifacts/);
  assert.match(chatRuntimeSource, /buildAggregatedAssistantMessages/);
  assert.doesNotMatch(chatRuntimeSource, /reasoningSegments\.join/);
});

test("assistant messages render turn artifacts outside inline assistant-ui parts", () => {
  assert.match(threadSource, /const AssistantTurnArtifacts: FC = \(\) => \{/);
  assert.match(threadSource, /const TurnTimelinePanel = \(\{/);
  assert.doesNotMatch(threadSource, /Reasoning:\s*ReasoningBlock/);
  assert.doesNotMatch(
    threadSource,
    /tools:\s*\{\s*Fallback:\s*ToolFallback\s*\}/,
  );
});

test("tool artifacts are rendered inline as timeline items instead of a separate tool list", () => {
  assert.match(threadSource, /turnArtifacts\.items\.map\(\(item\)/);
  assert.match(threadSource, /item\.type === "tool_call"/);
  assert.doesNotMatch(threadSource, /const ToolCallList = \(\{/);
  assert.doesNotMatch(threadSource, /调用了 \{toolCalls\.length\} 个工具/);
});
