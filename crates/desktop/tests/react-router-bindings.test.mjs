import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const routerPath = new URL("../router.tsx", import.meta.url);
const settingsPagePath = new URL("../app/settings/page.tsx", import.meta.url);
const newAgentPagePath = new URL("../app/settings/agents/new/page.tsx", import.meta.url);
const editAgentPagePath = new URL("../app/settings/agents/edit/page.tsx", import.meta.url);
const providersPagePath = new URL("../app/settings/providers/page.tsx", import.meta.url);
const editProviderPagePath = new URL("../app/settings/providers/edit/page.tsx", import.meta.url);
const agentEditorPath = new URL("../components/settings/agent-editor.tsx", import.meta.url);
const providerEditorPath = new URL("../components/settings/provider-editor.tsx", import.meta.url);
const providerCardPath = new URL("../components/settings/provider-card.tsx", import.meta.url);
const loginDialogPath = new URL("../components/auth/login-dialog.tsx", import.meta.url);
const dashboardShellPath = new URL(
  "../components/shadcn-studio/blocks/dashboard-shell-05/index.tsx",
  import.meta.url,
);

test("desktop exports a React Router route tree for the Tauri SPA", () => {
  assert.equal(existsSync(routerPath), true);
  const routerSource = readFileSync(routerPath, "utf8");

  assert.match(routerSource, /createBrowserRouter/);
  assert.match(routerSource, /path:\s*"settings"/);
  assert.match(routerSource, /path:\s*"providers"/);
  assert.match(routerSource, /path:\s*"agents"/);
  assert.match(routerSource, /path:\s*"knowledge"/);
  assert.match(routerSource, /path:\s*"tools"/);
});

test("settings routes use react-router redirects and query param hooks", () => {
  const settingsPageSource = readFileSync(settingsPagePath, "utf8");
  const newAgentPageSource = readFileSync(newAgentPagePath, "utf8");
  const editAgentPageSource = readFileSync(editAgentPagePath, "utf8");
  const editProviderPageSource = readFileSync(editProviderPagePath, "utf8");

  assert.match(settingsPageSource, /Navigate/);
  assert.match(settingsPageSource, /to="\/settings\/providers"/);
  assert.match(newAgentPageSource, /useSearchParams/);
  assert.match(editAgentPageSource, /useSearchParams/);
  assert.match(editProviderPageSource, /useSearchParams/);
  assert.doesNotMatch(newAgentPageSource, /next\/navigation/);
  assert.doesNotMatch(editAgentPageSource, /next\/navigation/);
  assert.doesNotMatch(editProviderPageSource, /next\/navigation/);
});

test("interactive desktop flows navigate through react-router instead of Next hooks", () => {
  const providersPageSource = readFileSync(providersPagePath, "utf8");
  const agentEditorSource = readFileSync(agentEditorPath, "utf8");
  const providerEditorSource = readFileSync(providerEditorPath, "utf8");
  const providerCardSource = readFileSync(providerCardPath, "utf8");
  const loginDialogSource = readFileSync(loginDialogPath, "utf8");
  const dashboardShellSource = readFileSync(dashboardShellPath, "utf8");

  assert.match(providersPageSource, /useNavigate/);
  assert.match(agentEditorSource, /useNavigate/);
  assert.match(providerEditorSource, /useNavigate/);
  assert.match(providerCardSource, /useNavigate/);
  assert.match(loginDialogSource, /useNavigate/);
  assert.match(dashboardShellSource, /Link/);
  assert.match(dashboardShellSource, /useLocation/);
  assert.doesNotMatch(providersPageSource, /next\/navigation/);
  assert.doesNotMatch(agentEditorSource, /next\/navigation/);
  assert.doesNotMatch(providerEditorSource, /next\/navigation/);
  assert.doesNotMatch(providerCardSource, /next\/navigation/);
  assert.doesNotMatch(loginDialogSource, /next\/navigation/);
  assert.doesNotMatch(dashboardShellSource, /next\/link|next\/navigation|next-themes/);
});
