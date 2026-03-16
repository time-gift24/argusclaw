import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const tauriSource = readFileSync(
  new URL("../lib/tauri.ts", import.meta.url),
  "utf8",
);
const providersPageSource = readFileSync(
  new URL("../app/settings/providers/page.tsx", import.meta.url),
  "utf8",
);
const providerCardSource = readFileSync(
  new URL("../components/settings/provider-card.tsx", import.meta.url),
  "utf8",
);
const providerDialogPath = new URL(
  "../components/settings/provider-test-dialog.tsx",
  import.meta.url,
);

test("desktop tauri bindings expose provider test connection types and invoke wrapper", () => {
  assert.match(tauriSource, /export type ProviderTestStatus =/);
  assert.match(tauriSource, /export interface ProviderTestResult/);
  assert.match(
    tauriSource,
    /testConnection:\s*\(id: string\)\s*=>\s*invoke<ProviderTestResult>\("test_provider_connection",\s*\{ id \}\)/,
  );
  assert.match(
    tauriSource,
    /testInput:\s*\(record: ProviderInput\)\s*=>\s*invoke<ProviderTestResult>\("test_provider_input",\s*\{ record \}\)/,
  );
});

test("providers page keeps transient provider test status state and wires the card actions", () => {
  assert.match(
    providersPageSource,
    /const \[testResultsByProviderId, setTestResultsByProviderId\] = React\.useState<\s*Record<string, ProviderTestResult>\s*>\(\{\}\)/,
  );
  assert.match(
    providersPageSource,
    /const \[activeProviderId, setActiveProviderId\] = React\.useState<string \| null>\(\s*null,\s*\)/,
  );
  assert.match(
    providersPageSource,
    /const \[testDialogOpen, setTestDialogOpen\] = React\.useState\(false\)/,
  );
  assert.match(
    providersPageSource,
    /const \[testingProviderId, setTestingProviderId\] = React\.useState<\s*string \| null\s*>\(null\)/,
  );
  assert.match(providersPageSource, /providers\.testConnection\(id\)/);
  assert.match(
    providersPageSource,
    /onTestConnection=\{handleTestConnection\}/,
  );
  assert.match(providersPageSource, /onViewStatus=\{handleViewStatus\}/);
  assert.match(providersPageSource, /<ProviderTestDialog/);
});

test("provider card exposes test connection and clickable status affordances", () => {
  assert.match(providerCardSource, /onTestConnection: \(id: string\) => void/);
  assert.match(providerCardSource, /onViewStatus: \(id: string\) => void/);
  assert.match(providerCardSource, /测试连接/);
  assert.match(providerCardSource, /查看状态/);
});

test("provider test dialog exists and exposes loading, result details, and retest controls", () => {
  assert.equal(existsSync(providerDialogPath), true);
  const providerDialogSource = readFileSync(providerDialogPath, "utf8");

  assert.match(providerDialogSource, /正在测试/);
  assert.match(providerDialogSource, /重新测试/);
  assert.match(providerDialogSource, /latency_ms/);
  assert.match(providerDialogSource, /checked_at/);
});
