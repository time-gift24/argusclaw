import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const newAgentPagePath = new URL(
  "../app/settings/agents/new/page.tsx",
  import.meta.url,
);
const agentEditorPath = new URL(
  "../components/settings/agent-editor.tsx",
  import.meta.url,
);
const providersPagePath = new URL(
  "../app/settings/providers/page.tsx",
  import.meta.url,
);
const providerDialogPath = new URL(
  "../components/settings/provider-form-dialog.tsx",
  import.meta.url,
);
const deleteDialogPath = new URL(
  "../components/settings/delete-confirm-dialog.tsx",
  import.meta.url,
);

test("settings exposes a dedicated new-agent route that renders create mode", () => {
  assert.equal(existsSync(newAgentPagePath), true);

  const newAgentPageSource = readFileSync(newAgentPagePath, "utf8");

  assert.match(newAgentPageSource, /<AgentEditor\s*\/>/);
  assert.doesNotMatch(newAgentPageSource, /agentId=/);
});

test("agent editor treats provider as optional when deciding whether the form can save", () => {
  const agentEditorSource = readFileSync(agentEditorPath, "utf8");
  const providerSelectBlock =
    agentEditorSource.match(
      /<select[\s\S]*?id="provider_id"[\s\S]*?<\/select>/,
    )?.[0] ?? "";

  assert.match(agentEditorSource, /function getPreferredProviderId/);
  assert.match(agentEditorSource, /const isEditing = agentId !== undefined/);
  assert.match(
    agentEditorSource,
    /providersData\.find\(\(p\)\s*=>\s*p\.is_default && p\.secret_status !== "requires_reentry"\)\?\.id[\s\S]*providersData\.find\(\(p\)\s*=>\s*p\.secret_status !== "requires_reentry"\)\?\.id[\s\S]*null/,
  );
  assert.match(
    agentEditorSource,
    /const canSave = Boolean\([\s\S]*formData\.display_name\.trim\(\)[\s\S]*formData\.system_prompt\.trim\(\)[\s\S]*\)/,
  );
  assert.match(
    agentEditorSource,
    /<Button size="sm" onClick=\{handleSave\} disabled=\{saving \|\| !canSave\}>/,
  );
  assert.match(agentEditorSource, /const savedId = await agents\.upsert\(formData\)/);
  assert.match(agentEditorSource, /router\.push\(`\/settings\/agents\/\$\{savedId\}`\)/);
  assert.doesNotMatch(providerSelectBlock, /required/);
});

test("provider editing flow uses dedicated routes while keeping dialog open state controllable", () => {
  const providersPageSource = readFileSync(providersPagePath, "utf8");
  const providerDialogSource = readFileSync(providerDialogPath, "utf8");

  assert.match(providersPageSource, /router\.push\("\/settings\/providers\/new"\)/);
  assert.match(providerDialogSource, /open\?: boolean/);
  assert.match(
    providerDialogSource,
    /onOpenChange\?: \(open: boolean\) => void/,
  );
  assert.match(
    providerDialogSource,
    /const handleOpenChange = React\.useCallback/,
  );
  assert.match(
    providerDialogSource,
    /<Dialog open=\{open\} onOpenChange=\{handleOpenChange\}>/,
  );
  assert.match(providerDialogSource, /secret_status:/);
  assert.match(providerDialogSource, /requires_reentry/);
  assert.match(providerDialogSource, /重新填写 API Key/);
});

test("provider form dialog can test the current draft configuration before saving", () => {
  const providerDialogSource = readFileSync(providerDialogPath, "utf8");

  assert.match(providerDialogSource, /providers,/);
  assert.match(providerDialogSource, /type ProviderInput,/);
  assert.match(providerDialogSource, /type ProviderTestResult,/);
  assert.match(
    providerDialogSource,
    /const \[testingConnection, setTestingConnection\] = React\.useState\(false\)/,
  );
  assert.match(
    providerDialogSource,
    /const \[testDialogOpen, setTestDialogOpen\] = React\.useState\(false\)/,
  );
  assert.match(
    providerDialogSource,
    /const \[testResult, setTestResult\] = React\.useState<ProviderTestResult \| null>\(\s*null,\s*\)/,
  );
  assert.match(providerDialogSource, /providers\.testInput\(record, record\.default_model\)/);
  assert.match(providerDialogSource, /测试连接/);
  assert.match(providerDialogSource, /<ProviderTestDialog/);
});

test("provider cards and agent editor surface providers that require api key reentry", () => {
  const providersPageSource = readFileSync(providersPagePath, "utf8");
  const providerDialogSource = readFileSync(providerDialogPath, "utf8");
  const agentEditorSource = readFileSync(agentEditorPath, "utf8");

  assert.match(providersPageSource, /secret_status/);
  assert.match(providersPageSource, /requires_reentry/);
  assert.match(providerDialogSource, /secret_status === "requires_reentry"/);
  assert.match(agentEditorSource, /secret_status === "requires_reentry"/);
  assert.match(agentEditorSource, /disabled=\{.*secret_status === "requires_reentry"/);
});


test("delete confirmation dialog keeps the modal open and surfaces backend errors", () => {
  const deleteDialogSource = readFileSync(deleteDialogPath, "utf8");

  assert.match(deleteDialogSource, /const \[errorMessage, setErrorMessage\] = React\.useState\(""\)/);
  assert.match(deleteDialogSource, /await onConfirm\(\)/);
  assert.match(deleteDialogSource, /setErrorMessage\(message\)/);
  assert.match(deleteDialogSource, /<p className="text-sm text-destructive">\{errorMessage\}<\/p>/);
});
