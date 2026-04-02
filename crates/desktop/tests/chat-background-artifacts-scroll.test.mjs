import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const threadSource = readFileSync(
  new URL("../components/assistant-ui/thread.tsx", import.meta.url),
  "utf8",
);
const toolFallbackSource = readFileSync(
  new URL("../components/assistant-ui/tool-fallback.tsx", import.meta.url),
  "utf8",
);

test("background job artifacts stay scrollable inside the detached composer area", () => {
  assert.match(
    threadSource,
    /<div className="mt-3 max-h-\[[^\"]+\] overflow-y-auto custom-scrollbar pr-1">/,
  );
});

test("pending tool artifacts stay scrollable inside the detached composer area", () => {
  assert.match(
    threadSource,
    /<div className="mt-2 flex max-h-\[[^\"]+\] flex-col gap-1 overflow-y-auto custom-scrollbar border-l-2 border-muted\/30 pl-4 pr-1 ml-4">/,
  );
});

test("tool fallback payload blocks cap their own height and expose inner scrolling", () => {
  const argsStart = toolFallbackSource.indexOf("function ToolFallbackArgs");
  const resultStart = toolFallbackSource.indexOf("function ToolFallbackResult");
  const errorStart = toolFallbackSource.indexOf("function ToolFallbackError");

  assert.notEqual(argsStart, -1);
  assert.notEqual(resultStart, -1);
  assert.notEqual(errorStart, -1);

  const argsBlock = toolFallbackSource.slice(argsStart, resultStart);
  const resultBlock = toolFallbackSource.slice(resultStart, errorStart);

  assert.match(
    argsBlock,
    /max-h-\[[^\]]+\]\s+overflow-auto\s+custom-scrollbar/,
  );
  assert.match(
    resultBlock,
    /max-h-\[[^\]]+\]\s+overflow-auto\s+custom-scrollbar/,
  );
});
