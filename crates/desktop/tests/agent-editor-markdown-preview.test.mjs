import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const agentEditorSource = readFileSync(
  new URL("../components/settings/agent-editor.tsx", import.meta.url),
  "utf8",
);

test("agent editor preview uses assistant-ui markdown parts directly", () => {
  assert.match(
    agentEditorSource,
    /import\s+\{\s*MarkdownText\s*\}\s+from\s+"@\/components\/assistant-ui\/markdown-text"/,
  );
  assert.match(
    agentEditorSource,
    /import\s+\{\s*MessageProvider,\s*MessagePrimitive,\s*type ThreadAssistantMessage\s*\}\s+from\s+"@assistant-ui\/react"/,
  );
  assert.match(
    agentEditorSource,
    /<MessagePrimitive\.Parts\s+components=\{\{\s*Text:\s*MarkdownText\s*\}\}\s*\/>/,
  );
  assert.match(agentEditorSource, /<MessageProvider\s+message=\{previewMessage\}\s+index=\{0\}\s+isLast>/);
  assert.match(agentEditorSource, /<TabsContent value="preview"/);
  assert.match(agentEditorSource, /<div className="prose prose-sm dark:prose-invert max-w-none">/);
  assert.doesNotMatch(agentEditorSource, /from\s+"react-markdown"/);
});

test("agent editor no longer uses the raw markdown preview helper", () => {
  assert.equal(
    existsSync(new URL("../components/settings/agent-prompt-preview.tsx", import.meta.url)),
    false,
  );
});
