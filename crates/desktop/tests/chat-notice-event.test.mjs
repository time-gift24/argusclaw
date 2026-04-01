import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const chatTypesSource = readFileSync(
  new URL("../lib/types/chat.ts", import.meta.url),
  "utf8",
);
const storeSource = readFileSync(new URL("../lib/chat-store.ts", import.meta.url), "utf8");

test("desktop thread event types expose notice payloads", () => {
  assert.match(
    chatTypesSource,
    /export type ThreadNoticeLevel = "info" \| "warning" \| "error"/,
    "thread event types should expose the shared notice levels",
  );
  assert.match(
    chatTypesSource,
    /type: "notice";\s*level: ThreadNoticeLevel;\s*message: string/,
    "thread event payload union should include notice events",
  );
});

test("chat store handles notice events without disrupting streaming state", () => {
  const noticeBranch = storeSource.match(/case "notice":(?<branch>[\s\S]*?)break;/);

  assert.ok(noticeBranch?.groups?.branch, "notice branch should exist");
  assert.match(
    noticeBranch.groups.branch,
    /payload\.level === "warning" \|\| payload\.level === "error"/,
    "warning and error notices should be surfaced to the user",
  );
  assert.match(
    noticeBranch.groups.branch,
    /errorMessage:\s*payload\.message/,
    "surface notices through the shared error message channel",
  );
  assert.doesNotMatch(
    noticeBranch.groups.branch,
    /status:|pendingAssistant:\s*null/,
    "notice handling should not reset session execution state",
  );
});
