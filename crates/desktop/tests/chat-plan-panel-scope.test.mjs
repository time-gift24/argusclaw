import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const threadSource = readFileSync(
  new URL("../components/assistant-ui/thread.tsx", import.meta.url),
  "utf8",
);

test("plan panel rendering does not depend on assistant-ui part scope", () => {
  assert.doesNotMatch(threadSource, /s\.part\.status\.type === "running"/);
  assert.doesNotMatch(
    threadSource,
    /const AssistantMessage: FC = \(\) => \{[\s\S]*?pendingAssistant/s,
  );
});

test("pending assistant artifacts render outside assistant message roots", () => {
  assert.match(threadSource, /const PendingAssistantArtifacts: FC = \(\) => \{/);
  assert.match(
    threadSource,
    /<ThreadPrimitive\.Messages[\s\S]*?\/>\s*\n\s*<PendingAssistantArtifacts \/>/,
  );
  assert.doesNotMatch(
    threadSource,
    /const AssistantMessage: FC = \(\) => \{\s*const session = useActiveChatSession\(\);/s,
  );
});
