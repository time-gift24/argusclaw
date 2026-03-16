import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const threadSource = readFileSync(
  new URL("../components/assistant-ui/thread.tsx", import.meta.url),
  "utf8",
);
const layoutSource = readFileSync(
  new URL("../app/layout.tsx", import.meta.url),
  "utf8",
);
const globalsSource = readFileSync(
  new URL("../app/globals.css", import.meta.url),
  "utf8",
);

test("thread keeps the scroll-to-bottom control inside the assistant-ui viewport", () => {
  const viewportOpen = threadSource.indexOf("<ThreadPrimitive.Viewport");
  const scrollButton = threadSource.indexOf("<ThreadPrimitive.ScrollToBottom");
  const viewportClose = threadSource.indexOf("</ThreadPrimitive.Viewport>");

  assert.notEqual(viewportOpen, -1);
  assert.notEqual(scrollButton, -1);
  assert.notEqual(viewportClose, -1);
  assert.ok(scrollButton > viewportOpen, "scroll button should come after the viewport opens");
  assert.ok(scrollButton < viewportClose, "scroll button should stay inside the viewport context");
  assert.match(threadSource, /aui-thread-root [^\"]*relative[^\"]*min-h-0/);
  assert.match(threadSource, /aui-thread-viewport [^\"]*min-h-0[^\"]*overflow-y-auto/);
  assert.match(threadSource, /sticky bottom-24 z-40 mx-auto/);
  assert.doesNotMatch(threadSource, /absolute bottom-24 left-1\/2 z-40/);
});

test("desktop layout constrains the chat surface to the Tauri window", () => {
  assert.match(layoutSource, /className=\"h-full antialiased font-sans\"/);
  assert.match(layoutSource, /<body className=\"flex h-dvh min-h-dvh flex-col overflow-hidden\">/);
  assert.match(layoutSource, /<main className=\"flex min-h-0 flex-1 flex-col overflow-y-auto\">/);
  assert.match(globalsSource, /html,\s*body\s*\{\s*height:\s*100%;/);
});
