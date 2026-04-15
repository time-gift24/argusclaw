import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const newAgentPagePath = new URL(
  "../app/settings/agents/new/page.tsx",
  import.meta.url,
);
const editAgentPagePath = new URL(
  "../app/settings/agents/edit/page.tsx",
  import.meta.url,
);
const legacyAgentPagePath = new URL(
  "../app/settings/agents/[id]/page.tsx",
  import.meta.url,
);
const editProviderPagePath = new URL(
  "../app/settings/providers/edit/page.tsx",
  import.meta.url,
);
const legacyProviderPagePath = new URL(
  "../app/settings/providers/[id]/page.tsx",
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
const agentsPagePath = new URL(
  "../app/settings/agents/page.tsx",
  import.meta.url,
);
const providerCardPath = new URL(
  "../components/settings/provider-card.tsx",
  import.meta.url,
);
const providerEditorPath = new URL(
  "../components/settings/provider-editor.tsx",
  import.meta.url,
);
const loginDialogPath = new URL(
  "../components/auth/login-dialog.tsx",
  import.meta.url,
);
const settingsLayoutPath = new URL(
  "../app/settings/layout.tsx",
  import.meta.url,
);

test("settings exposes a dedicated new-agent route that renders create mode", () => {
  assert.equal(existsSync(newAgentPagePath), true);

  const newAgentPageSource = readFileSync(newAgentPagePath, "utf8");

  assert.match(newAgentPageSource, /<AgentEditor[\s\S]*\/>/);
  assert.doesNotMatch(newAgentPageSource, /agentId=/);
});

test("settings uses static edit pages instead of dynamic detail routes for export builds", () => {
  assert.equal(existsSync(editAgentPagePath), true);
  assert.equal(existsSync(editProviderPagePath), true);
  assert.equal(existsSync(legacyAgentPagePath), false);
  assert.equal(existsSync(legacyProviderPagePath), false);

  const editAgentPageSource = readFileSync(editAgentPagePath, "utf8");
  const editProviderPageSource = readFileSync(editProviderPagePath, "utf8");

  assert.match(editAgentPageSource, /useSearchParams/);
  assert.match(editAgentPageSource, /<AgentEditor agentId=\{agentId\} \/>/);
  assert.match(editProviderPageSource, /useSearchParams/);
  assert.match(editProviderPageSource, /<ProviderEditor providerId=\{providerId\} \/>/);
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
    /<Button[\s\S]*size="sm"[\s\S]*onClick=\{handleSave\}[\s\S]*disabled=\{saving \|\| !canSave\}[\s\S]*>/,
  );
  assert.match(
    agentEditorSource,
    /const cleanedFormData = ensureSchedulerToolState\(\{[\s\S]*subagent_names: formData\.subagent_names\.filter\([\s\S]*!missingSubagentNames\.includes\(name\)[\s\S]*\}, schedulerExplicitlySelected\)/,
  );
  assert.match(agentEditorSource, /setFormData\(cleanedFormData\)/);
  assert.match(agentEditorSource, /const savedId = await agents\.upsert\(cleanedFormData\)/);
  assert.match(agentEditorSource, /navigate\(`\/settings\/agents\/edit\?id=\$\{savedId\}`\)/);
  assert.doesNotMatch(providerSelectBlock, /required/);
});

test("agent editor auto-enables scheduler when subagents are configured", () => {
  const agentEditorSource = readFileSync(agentEditorPath, "utf8");

  assert.match(
    agentEditorSource,
    /const schedulerRequired = formData\.subagent_names\.length > 0/,
  );
  assert.match(
    agentEditorSource,
    /const isLockedScheduler = isSchedulerTool && schedulerRequired/,
  );
  assert.match(
    agentEditorSource,
    /const cleanedFormData = ensureSchedulerToolState\(\{[\s\S]*subagent_names: formData\.subagent_names\.filter\([\s\S]*\}, schedulerExplicitlySelected\)/,
  );
  assert.match(agentEditorSource, /disabled=\{isLockedScheduler\}/);
  assert.match(agentEditorSource, /因子代理配置自动启用/);
});

test("settings layout keeps edit pages inside a shrinkable scroll container", () => {
  const settingsLayoutSource = readFileSync(settingsLayoutPath, "utf8");
  const agentEditorSource = readFileSync(agentEditorPath, "utf8");

  assert.match(
    settingsLayoutSource,
    /className="flex min-h-0 flex-1 flex-col overflow-y-auto"/,
  );
  assert.match(
    settingsLayoutSource,
    /className="mx-auto w-full max-w-7xl px-4 py-3"/,
  );
  assert.match(
    agentEditorSource,
    /className="w-full h-full flex flex-col min-h-0 animate-in fade-in duration-500 overflow-hidden"/,
  );
  assert.match(
    agentEditorSource,
    /className="flex-1 overflow-y-auto custom-scrollbar px-1 py-8"/,
  );
});

test("provider editing flow uses dedicated routes while keeping dialog open state controllable", () => {
  const providersPageSource = readFileSync(providersPagePath, "utf8");
  const providerDialogSource = readFileSync(providerDialogPath, "utf8");
  const providerCardSource = readFileSync(providerCardPath, "utf8");
  const loginDialogSource = readFileSync(loginDialogPath, "utf8");
  const agentsPageSource = readFileSync(agentsPagePath, "utf8");

  assert.match(providersPageSource, /navigate\("\/settings\/providers\/new"\)/);
  assert.match(providerCardSource, /navigate\(`\/settings\/providers\/edit\?id=\$\{provider\.id\}`\)/);
  assert.match(loginDialogSource, /navigate\('\/settings\/providers\/edit\?id=1'\)/);
  assert.match(agentsPageSource, /navigate\(`\/settings\/agents\/edit\?id=\$\{id\}`\)/);
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

test("provider editor allows connection tests when account token auth is enabled", () => {
  const providerEditorSource = readFileSync(providerEditorPath, "utf8");

  assert.match(
    providerEditorSource,
    /const canRunConnectionTest = Boolean\([\s\S]*formData\.api_key\.trim\(\)[\s\S]*formData\.meta_data\.account_token_source === "true"[\s\S]*\)/,
  );
  assert.match(
    providerEditorSource,
    /if \(formData\.base_url\.trim\(\) && canRunConnectionTest\) \{/,
  );
  assert.match(
    providerEditorSource,
    /disabled=\{isTesting \|\| !canRunConnectionTest\}/,
  );
});


test("delete confirmation dialog keeps the modal open and surfaces backend errors", () => {
  const deleteDialogSource = readFileSync(deleteDialogPath, "utf8");

  assert.match(deleteDialogSource, /const \[errorMessage, setErrorMessage\] = React\.useState\(""\)/);
  assert.match(deleteDialogSource, /await onConfirm\(\)/);
  assert.match(deleteDialogSource, /setErrorMessage\(message\)/);
  assert.match(deleteDialogSource, /<p className="text-sm text-destructive">\{errorMessage\}<\/p>/);
});
