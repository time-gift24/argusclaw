import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const storeSource = readFileSync(
  new URL("../lib/chat-store.ts", import.meta.url),
  "utf8",
);

test("subagent job detail store helpers exist and are used by event handling", () => {
  assert.match(storeSource, /const JOB_DETAIL_/);
  assert.match(storeSource, /normalizeJobDetailPayload/);
  assert.match(storeSource, /appendJobDetailTimelineEntry/);
  assert.match(storeSource, /findSessionKeyForEnvelope/);
  assert.match(storeSource, /mailbox_message_queued/);
  assert.match(storeSource, /message_type\.type === "job_result"/);
  assert.match(storeSource, /result_text:/);
  assert.match(storeSource, /timeline:/);
  assert.match(storeSource, /selectedJobDetailId:\s*null/);
});

test("subagent job detail selection actions exist", () => {
  assert.match(storeSource, /openJobDetails:\s*\(/);
  assert.match(storeSource, /closeJobDetails:\s*\(/);
  assert.match(storeSource, /selectedJobDetailId:/);
});

test("thread-pool job timeline updates resolve the parent session by session id and job id", () => {
  assert.match(
    storeSource,
    /session\.sessionId === envelope\.session_id[\s\S]*session\.jobDetails\[jobId\]/,
  );
});
