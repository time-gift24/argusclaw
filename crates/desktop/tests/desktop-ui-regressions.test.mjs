import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const globalsSource = readFileSync(
  new URL("../app/globals.css", import.meta.url),
  "utf8",
);
const mcpTestDialogSource = readFileSync(
  new URL("../components/settings/mcp-test-dialog.tsx", import.meta.url),
  "utf8",
);
const toolFallbackSource = readFileSync(
  new URL("../components/assistant-ui/tool-fallback.tsx", import.meta.url),
  "utf8",
);
const toolCardSource = readFileSync(
  new URL("../components/settings/tool-card.tsx", import.meta.url),
  "utf8",
);
const agentEditorSource = readFileSync(
  new URL("../components/settings/agent-editor.tsx", import.meta.url),
  "utf8",
);

test("MCP test dialog keeps a single visible close action", () => {
  assert.doesNotMatch(mcpTestDialogSource, /<DialogFooter\s+showCloseButton/);
  assert.match(
    mcpTestDialogSource,
    /<Button[\s\S]*onClick=\{\(\) => onOpenChange\(false\)\}[\s\S]*>\s*关闭\s*<\/Button>/,
  );
});

test("tool fallback collapse styling targets Base UI data attributes", () => {
  assert.match(toolFallbackSource, /group-data-\[panel-open\]\/trigger:rotate-0/);
  assert.match(toolFallbackSource, /group-not-data-\[panel-open\]\/trigger:-rotate-90/);
  assert.match(toolFallbackSource, /data-\[open\]:animate-collapsible-down/);
  assert.match(toolFallbackSource, /data-\[closed\]:animate-collapsible-up/);
  assert.match(toolFallbackSource, /工具参数/);
  assert.match(toolFallbackSource, /工具输出/);
  assert.doesNotMatch(toolFallbackSource, /参数 \(Arguments\)/);
  assert.doesNotMatch(toolFallbackSource, /输出 \(Output\)/);
  assert.doesNotMatch(toolFallbackSource, /data-\[state=/);
});

test("desktop keeps the global text scale compact across viewport sizes", () => {
  assert.match(globalsSource, /@apply font-sans text-sm;/);
  assert.doesNotMatch(globalsSource, /sm:text-base/);
});

test("settings tool cards use the shared collapsible primitive", () => {
  assert.match(toolCardSource, /from "@\/components\/ui\/collapsible"/);
  assert.match(toolCardSource, /<Collapsible[\s\S]*open=\{showParams\}/);
  assert.match(toolCardSource, /<CollapsibleTrigger[\s\S]*参数 schema/);
  assert.match(toolCardSource, /<CollapsibleContent/);
});

test("agent editor tool hover details stay scrollable under the pointer", () => {
  assert.match(agentEditorSource, /可用工具箱/);
  assert.match(agentEditorSource, /pointer-events-auto/);
  assert.match(agentEditorSource, /max-h-\[min\(24rem,55vh\)\]/);
  assert.match(agentEditorSource, /overflow-y-auto custom-scrollbar overscroll-contain/);
  assert.match(agentEditorSource, /onClick=\{\(event\) => event\.stopPropagation\(\)\}/);
  assert.doesNotMatch(agentEditorSource, /pointer-events-none absolute z-20 w-80/);
});
