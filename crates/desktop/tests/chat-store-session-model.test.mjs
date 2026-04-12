import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const storeSource = readFileSync(new URL("../lib/chat-store.ts", import.meta.url), "utf8");
const typesSource = readFileSync(
  new URL("../lib/types/chat.ts", import.meta.url),
  "utf8",
);

test("chat store keeps sessions keyed by template and provider preference", () => {
  assert.match(storeSource, /errorMessage:\s*string \| null/);
  assert.match(storeSource, /activeSessionKey:\s*string \| null/);
  assert.match(storeSource, /sessionsByKey:\s*Record<string,\s*ChatSessionState>/);
  assert.match(storeSource, /selectedProviderPreferenceId:\s*number \| null/);
  assert.match(storeSource, /threadPoolSnapshot:\s*ThreadPoolSnapshot \| null/);
  assert.match(storeSource, /threadPoolSnapshotLoading:\s*boolean/);
  assert.match(storeSource, /threadPoolError:\s*string \| null/);
  assert.match(storeSource, /threadPoolThreads:\s*ThreadPoolThreadState\[\]/);
  assert.match(storeSource, /refreshThreadPoolSnapshot:\s*\(\)\s*=>\s*Promise<void>/);
  assert.match(storeSource, /ThreadPoolRuntimeSummary/);
  assert.match(storeSource, /ThreadPoolRuntimeKind/);
  assert.match(storeSource, /threadRuntime\.getState\(/);
  assert.match(storeSource, /threadRuntime\.getState\(\)[\s\S]*threadPoolThreads:/);
  assert.match(storeSource, /mapRuntimeSummaryToThreadState/);
  assert.match(storeSource, /kind:\s*runtime\.runtime\.kind/);
  assert.match(storeSource, /sessionId:\s*runtime\.runtime\.session_id/);
  assert.match(storeSource, /jobId:\s*runtime\.runtime\.job_id/);
  assert.match(storeSource, /refreshSnapshot:\s*\([\s\S]*sessionKey:\s*string/);
  assert.match(storeSource, /listen[\s\S]*"thread:event"/);
  assert.match(storeSource, /thread_id|threadId/);
  assert.match(storeSource, /case "content_delta"/);
  assert.match(storeSource, /case "reasoning_delta"/);
  assert.match(storeSource, /case "llm_usage"/);
  assert.match(storeSource, /case "turn_completed"/);
  assert.match(storeSource, /case "job_dispatched"/);
  assert.match(storeSource, /case "job_result"/);
  assert.doesNotMatch(storeSource, /case "waiting_for_approval"/);
  assert.doesNotMatch(storeSource, /case "approval_resolved"/);
  assert.match(storeSource, /case "idle"/);
  assert.match(storeSource, /case "thread_bound_to_job"/);
  assert.match(storeSource, /case "thread_pool_queued"/);
  assert.match(storeSource, /case "thread_pool_started"/);
  assert.match(storeSource, /case "thread_pool_cooling"/);
  assert.match(storeSource, /case "thread_pool_evicted"/);
  assert.match(storeSource, /case "thread_pool_metrics_updated"/);
  assert.match(storeSource, /payload\.runtime\.kind/);
  assert.match(storeSource, /payload\.runtime\.session_id/);
  assert.match(storeSource, /payload\.runtime\.job_id/);
  assert.match(storeSource, /threadPoolThreads:\s*state\.threadPoolThreads\.map\(/);
  assert.match(storeSource, /void get\(\)\.refreshThreadPoolSnapshot\(\);/);
  assert.match(storeSource, /await get\(\)\.activateSession\(/);
  assert.match(storeSource, /chat\.createChatSession\(/);
  assert.match(storeSource, /chat\.getThreadSnapshot\(/);
  assert.match(storeSource, /catch \(error\)/);
  assert.match(storeSource, /errorMessage:/);
});

test("thread pool store keeps the full authoritative runtime list", () => {
  const sortHelper = storeSource.match(
    /function sortThreadPoolThreads\([\s\S]*?\n\}/,
  );
  assert.ok(sortHelper?.[0], "sort helper should exist");
  assert.doesNotMatch(
    sortHelper[0],
    /THREAD_POOL_RECENT_LIMIT|slice\(0,\s*THREAD_POOL_RECENT_LIMIT\)/,
    "authoritative pool state should not be truncated in the store layer",
  );
});

test("chat store guards thread-event listener registration against concurrent initialize calls", () => {
  assert.match(
    storeSource,
    /threadEventListenerInitPromise|listenerInitPromise|initializingThreadEventListener/,
    "store should track an in-flight listener registration promise",
  );
  assert.match(
    storeSource,
    /if\s*\(!get\(\)\._unlisten\)\s*\{[\s\S]*?await\s+.*threadEvent.*Promise/i,
    "initialize should await the shared listener registration instead of calling listen twice",
  );
});

test("chat store tracks pending reasoning alongside streamed assistant text", () => {
  assert.match(
    storeSource,
    /pendingAssistant:[\s\S]*content:\s*string;[\s\S]*reasoning:\s*string;[\s\S]*toolCalls:\s*PendingToolCall\[\];[\s\S]*plan:\s*PlanItem\[\]\s*\|\s*null[\s\S]*retry:[\s\S]*attempt:\s*number;[\s\S]*maxRetries:\s*number;[\s\S]*error:\s*string[\s\S]*\|\s*null[\s\S]*\}\s*\|\s*null/,
  );
  assert.match(
    storeSource,
    /case "reasoning_delta":[\s\S]*?ensurePendingAssistantSession\(session\)[\s\S]*?pendingAssistant:[\s\S]*?reasoning:\s*sessionWithPending\.pendingAssistant\.reasoning \+ payload\.delta/,
  );
});

test("chat store bootstraps pending assistant state for mailbox-triggered wakeups", () => {
  assert.match(
    storeSource,
    /const ensurePendingAssistantSession = \(\s*session: ChatSessionState,\s*\):[\s\S]*pendingAssistant:\s*PendingAssistantState[\s\S]*=> \(\{/,
    "store should expose a helper that can initialize pending assistant state outside manual sends",
  );
  assert.match(
    storeSource,
    /const createPendingAssistant = \(\): PendingAssistantState => \(\{[\s\S]*retry:\s*null[\s\S]*\}\);[\s\S]*const ensurePendingAssistantSession = \(/,
    "bootstrapped wakeups should create a full pending assistant shell when needed",
  );
  assert.match(
    storeSource,
    /const ensurePendingAssistantSession = \(\s*session: ChatSessionState,\s*\):[\s\S]*=> \(\{[\s\S]*status:\s*"running"/,
    "bootstrapped wakeups should surface as a running session",
  );
  assert.match(
    storeSource,
    /case "reasoning_delta":[\s\S]*ensurePendingAssistantSession\(session\)/,
    "reasoning deltas should create pending assistant state when a mailbox-triggered turn starts",
  );
  assert.match(
    storeSource,
    /case "content_delta":[\s\S]*ensurePendingAssistantSession\(session\)/,
    "content deltas should create pending assistant state when a mailbox-triggered turn starts",
  );
  assert.match(
    storeSource,
    /case "tool_started":[\s\S]*ensurePendingAssistantSession\(session\)/,
    "tool_started should create pending assistant state when a mailbox-triggered turn starts with a tool",
  );
});

test("chat store keeps an optimistic pending user message until the persisted snapshot catches up", () => {
  assert.match(
    storeSource,
    /pendingUserMessage:\s*string \| null/,
  );
  assert.match(
    storeSource,
    /async sendMessage\(content: string\) \{[\s\S]*?pendingUserMessage:\s*trimmedContent/,
  );
  assert.match(
    storeSource,
    /refreshSnapshot:[\s\S]*?pendingUserMessage:\s*null/,
  );
  assert.match(
    storeSource,
    /case "turn_failed":[\s\S]*?pendingUserMessage:\s*null/,
  );
});

test("chat store surfaces retry attempts on the pending assistant and clears them once output resumes", () => {
  assert.match(
    storeSource,
    /case "retry_attempt":[\s\S]*?pendingAssistant:[\s\S]*?retry:\s*\{[\s\S]*attempt:\s*payload\.attempt,[\s\S]*maxRetries:\s*payload\.max_retries,[\s\S]*error:\s*payload\.error[\s\S]*\}/,
  );
  assert.match(
    storeSource,
    /case "content_delta":[\s\S]*?retry:\s*null/,
  );
  assert.match(
    storeSource,
    /case "reasoning_delta":[\s\S]*?retry:\s*null/,
  );
  assert.match(
    storeSource,
    /case "tool_call_delta":[\s\S]*?retry:\s*null/,
  );
});

test("chat store tracks ephemeral job status outside the transcript", () => {
  assert.match(
    storeSource,
    /jobStatuses:\s*Record<string,\s*JobStatusPayload>/,
    "session state should keep per-job status for temporary UI rendering",
  );
  assert.match(
    storeSource,
    /case "job_dispatched":[\s\S]*status:\s*"running"/,
    "job_dispatched should mark the job as running",
  );
  assert.match(
    storeSource,
    /case "job_result":[\s\S]*status:\s*payload\.success\s*\?\s*"completed"\s*:\s*"failed"/,
    "job_result should only update job status instead of appending transcript text",
  );
  assert.match(
    storeSource,
    /const normalizeJobStatusPayload = \(\s*payload: JobStatusPayload,\s*\): JobStatusPayload => \(\{/,
    "job status payloads should be normalized before entering store state",
  );
  assert.match(
    storeSource,
    /case "job_dispatched":[\s\S]*normalizeJobStatusPayload\(/,
    "job_dispatched should clamp oversized subagent payloads before storing them",
  );
  assert.match(
    storeSource,
    /case "job_result":[\s\S]*normalizeJobStatusPayload\(/,
    "job_result should clamp oversized subagent payloads before storing them",
  );
});

test("desktop types expose mailbox job result payloads and detail state", () => {
  assert.match(typesSource, /export interface JobDetailPayload/);
  assert.match(typesSource, /export interface JobDetailTimelineItem/);
  assert.match(typesSource, /type:\s*"job_result"/);
  assert.match(typesSource, /type:\s*"mailbox_message_queued"/);
  assert.match(typesSource, /type:\s*"task_assignment"/);
});

test("chat store keeps a separate selected job detail and detail records", () => {
  assert.match(storeSource, /selectedJobDetailId:\s*string \| null/);
  assert.match(storeSource, /jobDetails:\s*Record<string,\s*JobDetailPayload>/);
  assert.match(storeSource, /openJobDetails:/);
  assert.match(storeSource, /closeJobDetails:/);
  assert.match(storeSource, /case "mailbox_message_queued"/);
  assert.match(storeSource, /normalizeJobDetailPayload/);
  assert.match(storeSource, /appendJobDetailTimelineEntry/);
  assert.match(storeSource, /selectedJobDetailId:\s*null/);
});

test("chat store waits for idle before refreshing the persisted snapshot", () => {
  const turnCompletedBranch = storeSource.match(
    /case "turn_completed":(?<branch>[\s\S]*?)break;/,
  );
  assert.ok(turnCompletedBranch, "turn_completed branch should still exist for status handling");
  assert.doesNotMatch(
    turnCompletedBranch.groups?.branch ?? "",
    /refreshSnapshot\(sessionKey\)/,
    "turn_completed should not refresh snapshot before history is durable",
  );
  assert.match(
    storeSource,
    /case "idle":[\s\S]*?refreshSnapshot\(sessionKey\)/,
    "idle should trigger the final snapshot refresh",
  );
});

test("chat store updates token count from llm usage and final turn usage", () => {
  assert.match(
    storeSource,
    /case "llm_usage":[\s\S]*?tokenCount:\s*payload\.total_tokens/,
  );
  assert.match(
    storeSource,
    /case "turn_completed":[\s\S]*?tokenCount:\s*payload\.total_tokens/,
  );
  assert.doesNotMatch(
    storeSource,
    /context_token_count/,
  );
});

test("failed update_plan completions clear any optimistic pending plan", () => {
  assert.match(
    storeSource,
    /case "tool_started":[\s\S]*?payload\.tool_name === "update_plan"[\s\S]*?plan:\s*args\.plan/,
  );
  assert.match(
    storeSource,
    /case "tool_completed":[\s\S]*?payload\.tool_name === "update_plan"[\s\S]*?plan:\s*payload\.is_error\s*\?\s*null\s*:/,
  );
});

test("turn_failed refresh preserves the frontend error state", () => {
  assert.match(
    storeSource,
    /refreshSnapshot:\s*\([\s\S]*sessionKey:\s*string,[\s\S]*options\?\s*:\s*\{\s*preserveError\?\s*:\s*boolean\s*\}/,
  );
  assert.match(
    storeSource,
    /case "turn_failed":[\s\S]*?refreshSnapshot\(sessionKey,\s*\{\s*preserveError:\s*true\s*\}\)/,
  );
  assert.match(
    storeSource,
    /errorMessage:\s*options\?\.preserveError\s*\?\s*state\.errorMessage\s*:\s*null/,
  );
  assert.match(
    storeSource,
    /status:\s*options\?\.preserveError\s*\?\s*"error"\s*:\s*"idle"/,
  );
});

test("turn_failed clears any pending assistant state before snapshot refresh", () => {
  assert.match(
    storeSource,
    /case "turn_failed":[\s\S]*?status:\s*"error"[\s\S]*?pendingAssistant:\s*null[\s\S]*?refreshSnapshot\(sessionKey,\s*\{\s*preserveError:\s*true\s*\}\)/,
  );
});

test("store keeps new-session selection as a draft until the first send", () => {
  const initializeBranch = storeSource.match(
    /async initialize\(\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(initializeBranch?.groups?.branch, "initialize branch should exist");
  assert.doesNotMatch(
    initializeBranch.groups.branch,
    /createChatSession|activateSession/,
    "initialize should not create or activate sessions",
  );
  assert.match(
    initializeBranch.groups.branch,
    /selectedTemplateId:\s*state\.selectedTemplateId\s*\?\?\s*templateList\[0\]\?\.id\s*\?\?\s*null/,
    "initialize should seed the selected template without creating a session",
  );

  const providerBranch = storeSource.match(
    /async selectProviderPreference\(providerId: number \| null\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(providerBranch?.groups?.branch, "selectProviderPreference branch should exist");
  assert.doesNotMatch(
    providerBranch.groups.branch,
    /activateSession\(/,
    "changing provider preference should not auto-create a session",
  );

  const modelBranch = storeSource.match(
    /async selectModelOverride\(model: string \| null\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(modelBranch?.groups?.branch, "selectModelOverride branch should exist");
  assert.doesNotMatch(
    modelBranch.groups.branch,
    /activateSession\(/,
    "changing model preference should not auto-create a session",
  );

  const draftBranch = storeSource.match(
    /startNewSessionDraft:\s*\(templateId\?:\s*number\s*\|\s*null\)\s*=>\s*\{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(draftBranch?.groups?.branch, "draft session branch should exist");
  assert.match(
    draftBranch.groups.branch,
    /activeSessionKey:\s*null/,
    "starting a new-session draft should clear the active session",
  );
  assert.doesNotMatch(
    draftBranch.groups.branch,
    /createChatSession|activateSession/,
    "starting a new-session draft should not create a backend session",
  );

  const sendBranch = storeSource.match(
    /async sendMessage\(content: string\) \{(?<branch>[\s\S]*?)\n  \},/,
  );
  assert.ok(sendBranch?.groups?.branch, "sendMessage branch should exist");
  assert.match(
    sendBranch.groups.branch,
    /if \(!state\.activeSessionKey\)[\s\S]*await get\(\)\.activateSession\(fallbackTemplateId\)/,
    "first send should still materialize the draft as a real session",
  );
});
