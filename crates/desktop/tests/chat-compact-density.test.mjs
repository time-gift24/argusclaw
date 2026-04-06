import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const chatScreenSource = readFileSync(
  new URL("../components/chat/chat-screen.tsx", import.meta.url),
  "utf8",
);
const threadSource = readFileSync(
  new URL("../components/assistant-ui/thread.tsx", import.meta.url),
  "utf8",
);
const planPanelSource = readFileSync(
  new URL("../components/chat/plan-panel.tsx", import.meta.url),
  "utf8",
);
const bannerSource = readFileSync(
  new URL("../components/chat/chat-status-banner.tsx", import.meta.url),
  "utf8",
);

test("chat tab header is compacted while preserving tabs structure", () => {
  assert.match(
    chatScreenSource,
    /className="flex min-h-0 flex-1 flex-col gap-2 overflow-hidden"/,
  );
  assert.match(chatScreenSource, /<div className="px-3 pt-3">/);
  assert.match(chatScreenSource, /<TabsList className="h-8 bg-muted\/60 px-1 shadow-sm">/);
});

test("chat thread message spacing and composer footprint are compacted", () => {
  assert.match(
    threadSource,
    /--thread-max-width" as string\]: "68rem"/,
  );
  assert.match(
    threadSource,
    /--composer-max-width" as string\]: "56rem"/,
  );
  assert.match(
    threadSource,
    /className="aui-thread-viewport relative flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto scroll-smooth px-3 pt-3 pb-6 custom-scrollbar"/,
  );
  assert.match(
    threadSource,
    /className="aui-assistant-message-root[\s\S]*?py-4 px-1\.5/,
  );
  assert.match(
    threadSource,
    /className="aui-user-message-root[\s\S]*?gap-y-1\.5 px-1\.5 py-4/,
  );
  assert.match(
    threadSource,
    /aui-user-message-content wrap-break-word rounded-2xl bg-muted\/45 px-4 py-2\.5/,
  );
  assert.match(
    threadSource,
    /className="z-50 pointer-events-none flex justify-center pb-6 pt-3"/,
  );
  assert.match(
    threadSource,
    /className="aui-composer-input mb-1 max-h-40 min-h-12/,
  );
});

test("plan panel and chat status banner keep structure with reduced density", () => {
  assert.match(
    planPanelSource,
    /className="w-full rounded-2xl border border-muted\/60 bg-background\/95/,
  );
  assert.match(
    planPanelSource,
    /className="flex cursor-pointer items-center gap-2 select-none px-4 py-2\.5/,
  );
  assert.match(
    planPanelSource,
    /className="max-h-\[160px\] overflow-y-auto custom-scrollbar px-4 py-3/,
  );
  assert.match(
    bannerSource,
    /className="rounded-md border border-sky-300 bg-sky-50 px-2\.5 py-1\.5 text-xs text-sky-700"/,
  );
  assert.match(
    bannerSource,
    /className="rounded-md border border-destructive\/30 bg-destructive\/10 px-2\.5 py-1\.5 text-xs text-destructive"/,
  );
});
