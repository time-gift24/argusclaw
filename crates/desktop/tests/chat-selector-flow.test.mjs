import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const threadSource = readFileSync(new URL("../components/assistant-ui/thread.tsx", import.meta.url), "utf8");
const agentSelectorSource = readFileSync(new URL("../components/assistant-ui/agent-selector.tsx", import.meta.url), "utf8");
const providerSelectorSource = readFileSync(new URL("../components/assistant-ui/provider-selector.tsx", import.meta.url), "utf8");
const dropdownMenuSource = readFileSync(new URL("../components/ui/dropdown-menu.tsx", import.meta.url), "utf8");

test("thread composer exposes chat selectors without legacy approval affordances", () => {
  assert.match(threadSource, /AgentSelector/);
  assert.match(threadSource, /ProviderSelector/);
  assert.doesNotMatch(threadSource, /ApprovalPrompt/);
  // Stop generating button should be removed (not Stop generating aria-label)
  assert.doesNotMatch(threadSource, /Stop generating/);
});

test("composer action places new-session and history buttons on the far left", () => {
  assert.match(threadSource, /NewSessionButton/);
  assert.match(threadSource, /SessionHistoryButton/);
  const leftControls = threadSource.match(
    /<div className="flex items-center gap-1\.5 pl-1">(?<controls>[\s\S]*?)<\/div>/,
  );
  assert.ok(leftControls?.groups?.controls, "left control group should exist");
  const controls = leftControls.groups.controls;
  assert.ok(
    controls.indexOf("<NewSessionButton />") < controls.indexOf("<SessionHistoryButton />"),
    "new-session button should come before history button",
  );
  assert.ok(
    controls.indexOf("<SessionHistoryButton />") < controls.indexOf("<AgentSelector />"),
    "history button should stay to the left of the agent selector",
  );
});

test("composer action uses composer primitives for both send and cancel states", () => {
  assert.match(threadSource, /<ComposerPrimitive\.Send asChild>/);
  assert.match(threadSource, /<ComposerPrimitive\.Cancel asChild>/);
  assert.doesNotMatch(threadSource, /<ThreadPrimitive\.Cancel asChild>/);
});

test("agent selector uses the dialog trigger render prop expected by base-ui", () => {
  assert.match(agentSelectorSource, /<DialogTrigger render=\{/);
  assert.doesNotMatch(agentSelectorSource, /<DialogTrigger asChild>/);
});

test("agent selector updates template choice without auto-creating a session", () => {
  assert.match(agentSelectorSource, /const selectTemplate = useChatStore/);
  assert.match(agentSelectorSource, /void selectTemplate\(templateId\)/);
  assert.doesNotMatch(agentSelectorSource, /void activateSession\(templateId\)/);
});

test("agent selector prefers the active session template for display", () => {
  assert.match(agentSelectorSource, /const activeSession = useChatStore/);
  assert.match(
    agentSelectorSource,
    /const currentTemplateId = activeSession\?\.templateId \?\? selectedTemplateId/,
  );
  assert.match(
    agentSelectorSource,
    /templates\.find\(\(t\) => t\.id === currentTemplateId\)/,
  );
});

test("agent selector confirms before creating a new session for another agent", () => {
  assert.match(agentSelectorSource, /const activateSession = useChatStore/);
  assert.match(agentSelectorSource, /const \[pendingTemplateId, setPendingTemplateId\]/);
  assert.match(
    agentSelectorSource,
    /if \(activeSession && activeSession\.templateId !== templateId\)/,
  );
  assert.match(agentSelectorSource, /setPendingTemplateId\(templateId\)/);
  assert.match(agentSelectorSource, /setConfirmOpen\(true\)/);
  assert.match(agentSelectorSource, /需要新建会话才能生效/);
  assert.match(agentSelectorSource, /切换智能体需要新建会话/);
  assert.match(agentSelectorSource, /继续当前会话/);
  assert.match(agentSelectorSource, /新建会话/);
  assert.match(agentSelectorSource, /useChatStore\.getState\(\)\.selectTemplate\(pendingTemplateId\)/);
  assert.match(agentSelectorSource, /void activateSession\(pendingTemplateId\)/);
  assert.match(agentSelectorSource, /setPendingTemplateId\(null\)/);
  assert.match(agentSelectorSource, /setConfirmOpen\(false\)/);
});

test("agent selector declares callback hooks before any empty-template early return", () => {
  const earlyReturnIndex = agentSelectorSource.indexOf(
    "if (templates.length === 0) return null;",
  );
  const cancelCallbackIndex = agentSelectorSource.indexOf(
    "const handleCancelPendingSwitch = React.useCallback",
  );
  const confirmCallbackIndex = agentSelectorSource.indexOf(
    "const handleConfirmPendingSwitch = React.useCallback",
  );

  assert.ok(earlyReturnIndex >= 0, "empty-template guard should exist");
  assert.ok(cancelCallbackIndex >= 0, "cancel callback hook should exist");
  assert.ok(confirmCallbackIndex >= 0, "confirm callback hook should exist");
  assert.ok(
    cancelCallbackIndex < earlyReturnIndex,
    "callback hooks must be declared before the early return to keep hook order stable",
  );
  assert.ok(
    confirmCallbackIndex < earlyReturnIndex,
    "all callback hooks must be declared before the early return to keep hook order stable",
  );
});

test("dropdown menu trigger bridges radix-style asChild usage without leaking props to the DOM", () => {
  assert.match(providerSelectorSource, /<DropdownMenuTrigger asChild>/);
  assert.match(dropdownMenuSource, /asChild\?: boolean/);
  assert.match(dropdownMenuSource, /if \(asChild && React\.isValidElement\(children\)\)/);
  assert.match(dropdownMenuSource, /render=\{children\}/);
  assert.doesNotMatch(dropdownMenuSource, /<MenuPrimitive\.Trigger[^>]*asChild/);
});

test("provider selector prefers the active session provider for display", () => {
  assert.match(providerSelectorSource, /const activeSession = useChatStore/);
  assert.match(
    providerSelectorSource,
    /const currentProviderId =[\s\S]*activeSession\?\.effectiveProviderId \?\? selectedProviderPreferenceId/,
  );
  assert.match(
    providerSelectorSource,
    /providers\.find\(\(p\) => p\.id === currentProviderId\)/,
  );
});
