import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const tauriSource = readFileSync(new URL("../lib/tauri.ts", import.meta.url), "utf8");
const commandSource = readFileSync(new URL("../src-tauri/src/commands.rs", import.meta.url), "utf8");

test("desktop tauri bindings expose chat session and thread snapshot wrappers", () => {
  const threadPoolRuntimeSummaryBlock =
    tauriSource.match(/export interface ThreadPoolRuntimeSummary \{[\s\S]*?\n\}/)?.[0] ?? "";

  assert.match(tauriSource, /export interface ChatSessionPayload/);
  assert.match(tauriSource, /export interface ThreadSnapshotPayload/);
  assert.match(tauriSource, /export interface ThreadPoolSnapshot/);
  assert.match(tauriSource, /export interface ThreadPoolRuntimeSummary/);
  assert.match(tauriSource, /export interface ThreadPoolState/);
  assert.match(tauriSource, /export interface JobRuntimeSnapshot/);
  assert.match(tauriSource, /export interface JobRuntimeSummary/);
  assert.match(tauriSource, /export interface JobRuntimeState/);
  assert.match(threadPoolRuntimeSummaryBlock, /thread_id:\s*string/);
  assert.match(threadPoolRuntimeSummaryBlock, /session_id:\s*string \| null/);
  assert.doesNotMatch(threadPoolRuntimeSummaryBlock, /job_id:/);
  assert.doesNotMatch(tauriSource, /export interface ThreadPoolRuntimeRef/);
  assert.doesNotMatch(tauriSource, /runtime:\s*ThreadPoolRuntimeRef/);
  assert.match(tauriSource, /plan_item_count:\s*number/);
  assert.match(
    tauriSource,
    /createChatSession:\s*\(\s*templateId: number,\s*providerPreferenceId: number \| null,\s*model: string \| null,\s*\)\s*=>\s*invoke<ChatSessionPayload>\("create_chat_session"/,
    );
  assert.match(
    tauriSource,
    /getThreadSnapshot:\s*\(sessionId: string, threadId: string\)\s*=>\s*invoke<ThreadSnapshotPayload>\("get_thread_snapshot"/,
  );
  assert.doesNotMatch(tauriSource, /resolveApproval:\s*\(/);
  assert.match(
    tauriSource,
    /renameSession:\s*\(sessionId: string,\s*name: string\)\s*=>\s*invoke<void>\("rename_session"/,
  );
  assert.match(
    tauriSource,
    /renameThread:\s*\(sessionId: string,\s*threadId: string,\s*title: string\)\s*=>\s*invoke<void>\("rename_thread"/,
  );
  assert.match(
    tauriSource,
    /getSnapshot:\s*\(\)\s*=>\s*invoke<ThreadPoolSnapshot>\("get_thread_pool_snapshot"/,
  );
  assert.match(
    tauriSource,
    /getState:\s*\(\)\s*=>\s*invoke<ThreadPoolState>\("get_thread_pool_state"/,
  );
  assert.match(
    tauriSource,
    /getState:\s*\(\)\s*=>\s*invoke<JobRuntimeState>\("get_job_runtime_state"/,
  );
});

test("tauri chat session creation keeps unnamed sessions blank for id fallback rendering", () => {
  assert.match(commandSource, /\.create_session\(""\)/);
  assert.doesNotMatch(commandSource, /create_session\(&format!\("Chat-/);
  assert.match(commandSource, /get_thread_pool_snapshot/);
  assert.match(commandSource, /get_thread_pool_state/);
  assert.match(commandSource, /get_job_runtime_state/);
  assert.doesNotMatch(commandSource, /compact_agent_id: Option<String>/);
  assert.doesNotMatch(tauriSource, /compactAgentId/);
});
