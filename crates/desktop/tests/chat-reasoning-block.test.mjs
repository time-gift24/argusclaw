import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const threadSource = readFileSync(
  new URL("../components/assistant-ui/thread.tsx", import.meta.url),
  "utf8",
);

test("reasoning block uses a max height instead of fixed height", () => {
  assert.match(threadSource, /max-h-\[200px\]/);
  assert.doesNotMatch(threadSource, /className="[^"]*\sh-\[200px\][^"]*"/);
});

test("reasoning block keeps a collapsible header", () => {
  assert.match(threadSource, /<details className="group w-full"/);
  assert.match(threadSource, /<summary className=/);
  assert.match(threadSource, /思考中/);
});
