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
  assert.match(
    agentEditorSource,
    /providersData\.find\(\(p\)\s*=>\s*p\.is_default\)\?\.id \|\| providersData\[0\]\?\.id \|\| ""/,
  );
  assert.match(
    agentEditorSource,
    /const canSave = Boolean\(\s*formData\.id\.trim\(\)\s*&&\s*formData\.display_name\.trim\(\)\s*&&\s*formData\.system_prompt\.trim\(\),\s*\)/,
  );
  assert.match(
    agentEditorSource,
    /<Button size="sm" onClick=\{handleSave\} disabled=\{saving \|\| !canSave\}>/,
  );
  assert.doesNotMatch(providerSelectBlock, /required/);
});

test("provider editing flow controls the dialog from the page state", () => {
  const providersPageSource = readFileSync(providersPagePath, "utf8");
  const providerDialogSource = readFileSync(providerDialogPath, "utf8");

  assert.match(providersPageSource, /open=\{!!editingProvider\}/);
  assert.match(
    providersPageSource,
    /onOpenChange=\{\(open\)\s*=>\s*!open\s*&&\s*setEditingProvider\(null\)\}/,
  );
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
  assert.match(providerDialogSource, /providers\.testInput\(record\)/);
  assert.match(providerDialogSource, /测试连接/);
  assert.match(providerDialogSource, /<ProviderTestDialog/);
});
