import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const tauriSource = readFileSync(new URL("../lib/tauri.ts", import.meta.url), "utf8");

test("desktop tauri bindings expose chat session and thread snapshot wrappers", () => {
  assert.match(tauriSource, /export interface ChatSessionPayload/);
  assert.match(tauriSource, /export interface ThreadSnapshotPayload/);
  assert.match(
    tauriSource,
    /createChatSession:\s*\(templateId: string, providerPreferenceId: string \| null\)\s*=>\s*invoke<ChatSessionPayload>\("create_chat_session"/,
  );
  assert.match(
    tauriSource,
    /getThreadSnapshot:\s*\(runtimeAgentId: string, threadId: string\)\s*=>\s*invoke<ThreadSnapshotPayload>\("get_thread_snapshot"/,
  );
  assert.match(
    tauriSource,
    /resolveApproval:\s*\(\s*runtimeAgentId: string,\s*requestId: string,\s*decision: ApprovalDecision,\s*resolvedBy\?: string \| null/,
  );
});
