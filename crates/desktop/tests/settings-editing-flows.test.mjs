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
const newProviderPagePath = new URL(
  "../app/settings/providers/new/page.tsx",
  import.meta.url,
);
const editProviderPagePath = new URL(
  "../app/settings/providers/[id]/page.tsx",
  import.meta.url,
);
const agentsPagePath = new URL(
  "../app/settings/agents/page.tsx",
  import.meta.url,
);
const providerEditorPath = new URL(
  "../components/settings/provider-editor.tsx",
  import.meta.url,
);

test("settings exposes a dedicated new-agent route that renders create mode", () => {
  assert.equal(existsSync(newAgentPagePath), true);

  const newAgentPageSource = readFileSync(newAgentPagePath, "utf8");

  assert.match(newAgentPageSource, /<AgentEditor\s*\/>/);
  assert.doesNotMatch(newAgentPageSource, /agentId=/);
});

test("agent editor binds provider and model together when deciding whether the form can save", () => {
  const agentEditorSource = readFileSync(agentEditorPath, "utf8");
  const providerSelectBlock =
    agentEditorSource.match(
      /<select[\s\S]*?id="provider_id"[\s\S]*?<\/select>/,
    )?.[0] ?? "";
  const modelSelectBlock =
    agentEditorSource.match(
      /<select[\s\S]*?id="model"[\s\S]*?<\/select>/,
    )?.[0] ?? "";

  assert.match(agentEditorSource, /function getPreferredProviderId/);
  assert.match(agentEditorSource, /function getPreferredModel/);
  assert.match(
    agentEditorSource,
    /providersData\.find\(\(p\)\s*=>\s*p\.is_default && p\.secret_status !== "requires_reentry"\)\?\.id[\s\S]*providersData\.find\(\(p\)\s*=>\s*p\.secret_status !== "requires_reentry"\)\?\.id[\s\S]*""/,
  );
  assert.match(
    agentEditorSource,
    /const hasProviderModelBinding = Boolean\(formData\.provider_id \|\| formData\.model\)/,
  );
  assert.match(
    agentEditorSource,
    /const isProviderModelBindingValid = !hasProviderModelBinding \|\| Boolean\(formData\.provider_id && formData\.model\)/,
  );
  assert.match(
    agentEditorSource,
    /const canSave = Boolean\(\s*formData\.display_name\.trim\(\)\s*&&\s*formData\.system_prompt\.trim\(\)\s*&&\s*isProviderModelBindingValid,\s*\)/,
  );
  assert.match(
    agentEditorSource,
    /<Button size="sm" onClick=\{handleSave\} disabled=\{saving \|\| !canSave\}>/,
  );
  assert.doesNotMatch(agentEditorSource, /<Label htmlFor="id">ID<\/Label>/);
  assert.doesNotMatch(providerSelectBlock, /required/);
  assert.match(agentEditorSource, /<Label htmlFor="model">模型（可选）<\/Label>/);
  assert.match(modelSelectBlock, /disabled=\{!formData\.provider_id\}/);
  assert.match(
    agentEditorSource,
    /setFormData\(\(prev\) => \(\{\s*\.\.\.prev,\s*provider_id: "",\s*model: null/,
  );
});

test("settings exposes dedicated provider routes that render the shared provider editor", () => {
  assert.equal(existsSync(newProviderPagePath), true);
  assert.equal(existsSync(editProviderPagePath), true);

  const newProviderPageSource = readFileSync(newProviderPagePath, "utf8");
  const editProviderPageSource = readFileSync(editProviderPagePath, "utf8");

  assert.match(newProviderPageSource, /<ProviderEditor\s*\/>/);
  assert.doesNotMatch(newProviderPageSource, /providerId=/);
  assert.match(editProviderPageSource, /async function EditProviderPage/);
  assert.match(
    editProviderPageSource,
    /params:\s*Promise<\{\s*id:\s*string\s*\}>/,
  );
  assert.match(editProviderPageSource, /const\s*\{\s*id\s*\}\s*=\s*await params/);
  assert.match(
    editProviderPageSource,
    /const\s+providerId\s*=\s*safeDecodeRouteId\(id\)/,
  );
  assert.match(
    editProviderPageSource,
    /<ProviderEditor providerId=\{providerId\} \/>/,
  );
});

test("provider editor owns the draft test flow and uses a shared two-column shell", () => {
  const providersPageSource = readFileSync(providersPagePath, "utf8");
  const providerEditorSource = readFileSync(providerEditorPath, "utf8");

  assert.match(providersPageSource, /<Link href="\/settings\/providers\/new">/);
  assert.doesNotMatch(providersPageSource, /ProviderFormDialog/);
  assert.match(providerEditorSource, /providers,/);
  assert.match(providerEditorSource, /type ProviderInput,/);
  assert.match(providerEditorSource, /type ProviderTestResult,/);
  assert.match(
    providerEditorSource,
    /const \[testingConnection, setTestingConnection\] = React\.useState\(false\)/,
  );
  assert.match(
    providerEditorSource,
    /const \[testSelectedModel, setTestSelectedModel\] = React\.useState<string>\(""\)/,
  );
  assert.match(
    providerEditorSource,
    /const \[testResult, setTestResult\] = React\.useState<ProviderTestResult \| null>\(\s*null,\s*\)/,
  );
  assert.match(
    providerEditorSource,
    /providers\.testInput\(record,\s*selectedTestModel\)/,
  );
  assert.match(providerEditorSource, /<Label htmlFor="kind">Kind<\/Label>/);
  assert.match(providerEditorSource, /<select[\s\S]*id="kind"/);
  assert.match(providerEditorSource, /value="openai-compatible"/);
  assert.match(providerEditorSource, /model_config/);
  assert.match(providerEditorSource, /最大上下文/);
  assert.match(providerEditorSource, /context_length/);
  assert.match(providerEditorSource, /type="number"/);
  assert.doesNotMatch(providerEditorSource, /<Label htmlFor="id">ID<\/Label>/);
  assert.match(
    providerEditorSource,
    /const canSave = Boolean\(\s*formData\.display_name\.trim\(\)\s*&&\s*formData\.base_url\.trim\(\)\s*&&\s*formData\.api_key\.trim\(\)\s*&&\s*formData\.models\.length > 0\s*&&\s*formData\.default_model\.trim\(\),\s*\)/,
  );
  assert.match(providerEditorSource, /router\.push\("\/settings\/providers"\)/);
  assert.match(providerEditorSource, /className="w-full mx-auto max-w-7xl px-6 py-6 space-y-4"/);
  assert.match(providerEditorSource, /grid grid-cols-2 gap-6/);
  assert.match(providerEditorSource, /连接测试/);
});

test("provider cards and agent editor surface providers that require api key reentry", () => {
  const providersPageSource = readFileSync(providersPagePath, "utf8");
  const providerEditorSource = readFileSync(providerEditorPath, "utf8");
  const agentEditorSource = readFileSync(agentEditorPath, "utf8");

  assert.match(providersPageSource, /secret_status/);
  assert.match(providersPageSource, /requires_reentry/);
  assert.match(providerEditorSource, /secret_status === "requires_reentry"/);
  assert.match(agentEditorSource, /secret_status === "requires_reentry"/);
  assert.match(agentEditorSource, /disabled=\{.*secret_status === "requires_reentry"/);
});

test("settings list pages keep a full-width shell and agent templates are not hidden behind missing providers", () => {
  const providersPageSource = readFileSync(providersPagePath, "utf8");
  const agentsPageSource = readFileSync(agentsPagePath, "utf8");

  assert.match(
    providersPageSource,
    /className="w-full mx-auto max-w-7xl px-6 py-6 space-y-4"/,
  );
  assert.match(
    agentsPageSource,
    /className="w-full mx-auto max-w-7xl px-6 py-6 space-y-4"/,
  );
  assert.match(
    agentsPageSource,
    /router\.push\(`\/settings\/agents\/\$\{encodeURIComponent\(id\)\}`\)/,
  );
  assert.match(
    agentsPageSource,
    /providerList\.length === 0 \?[\s\S]*\) : null\}/,
  );
  assert.match(agentsPageSource, /agentList\.length === 0 \?/);
  assert.match(agentsPageSource, /providers=\{providerList\}/);
});
