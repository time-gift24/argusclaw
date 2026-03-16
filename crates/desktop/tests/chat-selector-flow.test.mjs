import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const threadSource = readFileSync(new URL("../components/assistant-ui/thread.tsx", import.meta.url), "utf8");
const approvalSource = readFileSync(new URL("../components/chat/approval-prompt.tsx", import.meta.url), "utf8");
const agentSelectorSource = readFileSync(new URL("../components/assistant-ui/agent-selector.tsx", import.meta.url), "utf8");

test("thread composer exposes chat selectors and approval affordances", () => {
  assert.match(threadSource, /AgentSelector/);
  assert.match(threadSource, /ProviderSelector/);
  assert.match(threadSource, /ApprovalPrompt/);
  // Stop generating button should be removed (not Stop generating aria-label)
  assert.doesNotMatch(threadSource, /Stop generating/);
  assert.match(approvalSource, /resolveApproval/);
  assert.match(approvalSource, /批准|拒绝/);
});

test("agent selector uses the dialog trigger render prop expected by base-ui", () => {
  assert.match(agentSelectorSource, /<DialogTrigger render=\{/);
  assert.doesNotMatch(agentSelectorSource, /<DialogTrigger asChild>/);
});
