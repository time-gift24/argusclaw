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

test("settled assistant turns aggregate reasoning and tool artifacts into message metadata", () => {
  assert.match(chatRuntimeSource, /type TurnArtifacts = \{/);
  assert.match(chatRuntimeSource, /turnArtifacts/);
  assert.match(chatRuntimeSource, /buildAggregatedAssistantMessages/);
});

test("assistant messages render turn artifacts outside inline assistant-ui parts", () => {
  assert.match(threadSource, /const AssistantTurnArtifacts: FC = \(\) => \{/);
  assert.match(threadSource, /const TurnArtifactsPanel = \(\{/);
  assert.doesNotMatch(threadSource, /Reasoning:\s*ReasoningBlock/);
  assert.doesNotMatch(threadSource, /tools:\s*\{\s*Fallback:\s*ToolFallback\s*\}/);
});

test("tool artifacts are rendered as a per-turn row list instead of a count summary", () => {
  assert.match(threadSource, /const ToolCallList = \(\{/);
  assert.match(threadSource, /toolCalls\.map\(\(toolCall\)/);
  assert.doesNotMatch(threadSource, /调用了 \{toolCalls\.length\} 个工具/);
});
