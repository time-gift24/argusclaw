import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const chatRuntimeSource = readFileSync(
  new URL("../lib/chat-runtime.ts", import.meta.url),
  "utf8",
);
const chatScreenSource = readFileSync(
  new URL("../components/chat/chat-screen.tsx", import.meta.url),
  "utf8",
);
const threadMonitorScreenSource = readFileSync(
  new URL(
    "../components/thread-monitor/thread-monitor-screen.tsx",
    import.meta.url,
  ),
  "utf8",
);
const pageSource = readFileSync(
  new URL("../app/page.tsx", import.meta.url),
  "utf8",
);
const threadSource = readFileSync(
  new URL("../components/assistant-ui/thread.tsx", import.meta.url),
  "utf8",
);

test("chat screen wires assistant-ui runtime into the thread UI", () => {
  assert.match(chatRuntimeSource, /useExternalStoreRuntime/);
  assert.match(chatRuntimeSource, /pendingAssistant/);
  assert.match(chatRuntimeSource, /pendingUserMessage/);
  assert.match(chatRuntimeSource, /type TurnArtifacts = \{/);
  assert.match(chatRuntimeSource, /msg\.reasoning_content/);
  assert.match(chatRuntimeSource, /tool_calls|toolCallId/);
  assert.match(chatRuntimeSource, /buildAggregatedAssistantMessages/);
  assert.match(chatRuntimeSource, /result:\s*parseMessageContent/);
  assert.match(chatRuntimeSource, /turnArtifacts/);
  assert.match(chatRuntimeSource, /pending-user-/);
  assert.match(chatRuntimeSource, /createEmptyAssistantMetadata/);
  assert.match(
    chatRuntimeSource,
    /const onNew = React\.useCallback|const onNew = useCallback|onNew:/,
  );
  assert.match(chatScreenSource, /AssistantRuntimeProvider/);
  assert.match(chatScreenSource, /useChatRuntime\(\)/);
  assert.match(chatScreenSource, /TabsTrigger value="chat"/);
  assert.match(chatScreenSource, /TabsTrigger value="threads"/);
  assert.match(
    chatScreenSource,
    /TabsContent[\s\S]*value="chat"[\s\S]*className="m-0 flex min-h-0 flex-1 overflow-hidden"/,
  );
  assert.match(chatScreenSource, /TabsContent[\s\S]*value="threads"/);
  assert.match(chatScreenSource, /ThreadMonitorScreen/);
  assert.match(threadMonitorScreenSource, /冷却中/);
  assert.match(threadMonitorScreenSource, /已驱逐/);
  assert.match(
    threadMonitorScreenSource,
    /kindFilter|全部类型|线程监控|监控优先/,
  );
  assert.doesNotMatch(threadMonitorScreenSource, /Thread Monitor/);
  assert.match(threadMonitorScreenSource, /权威|authoritative/i);
  assert.match(chatScreenSource, /<Thread \/>/);
  assert.match(
    threadSource,
    /className="aui-root aui-thread-root @container relative flex h-full min-h-0 w-full flex-1 flex-col bg-background overflow-hidden"/,
  );
  assert.match(threadSource, /ChatStatusBanner/);
  assert.match(threadSource, /jobStatuses|JobStatus/);
  assert.match(
    threadSource,
    /whitespace-pre-wrap break-words[\s\S]*job\.message/,
  );
  assert.match(threadSource, /SubagentJobDetailsDrawer/);
  assert.match(threadSource, /openJobDetails/);
  assert.match(threadSource, /查看详情/);
  assert.match(threadSource, /AssistantTurnArtifacts/);
  assert.match(threadSource, /TurnTimelinePanel/);
  assert.match(threadSource, /pendingAssistant\.reasoning/);
  assert.match(threadSource, /pendingAssistant\.timeline/);
  assert.match(threadSource, /pendingAssistant\.retry/);
  assert.match(threadSource, /正在重试请求/);
  assert.match(pageSource, /<ChatScreen \/>/);
});

test("thread job actions keep a single detail trigger path", () => {
  assert.doesNotMatch(threadSource, /const handleOpenJobDetails/);
  assert.equal((threadSource.match(/查看详情/g) ?? []).length, 1);
});

test("pending runtime messages use stable synthetic timestamps", () => {
  assert.match(
    chatRuntimeSource,
    /function buildSyntheticMessageDate\(seed: number\): Date \{/,
  );
  assert.match(
    chatRuntimeSource,
    /buildPendingUserMessage\([\s\S]*createdAt:\s*buildSyntheticMessageDate\(/,
  );
  assert.match(
    chatRuntimeSource,
    /if \(session\.pendingAssistant\) \{[\s\S]*createdAt:\s*buildSyntheticMessageDate\(/,
  );
  assert.doesNotMatch(
    chatRuntimeSource,
    /buildPendingUserMessage\([\s\S]*createdAt:\s*new Date\(\)/,
  );
  assert.doesNotMatch(
    chatRuntimeSource,
    /if \(session\.pendingAssistant\) \{[\s\S]*createdAt:\s*new Date\(\)/,
  );
});

test("chat runtime memoizes external-store inputs to avoid feedback loops", () => {
  assert.match(
    chatRuntimeSource,
    /import \* as React from "react";|from "react"/,
  );
  assert.match(
    chatRuntimeSource,
    /const messages = React\.useMemo\([\s\S]*buildAggregatedAssistantMessages\(session\)[\s\S]*\[session\][\s\S]*\);|const messages = useMemo\([\s\S]*buildAggregatedAssistantMessages\(session\)[\s\S]*\[session\][\s\S]*\);/,
  );
  assert.match(
    chatRuntimeSource,
    /const convertMessage = React\.useCallback\([\s\S]*message[\s\S]*=>[\s\S]*message[\s\S]*\[\][\s\S]*\);|const convertMessage = useCallback\([\s\S]*message[\s\S]*=>[\s\S]*message[\s\S]*\[\][\s\S]*\);/,
  );
  assert.match(
    chatRuntimeSource,
    /const onNew = React\.useCallback\([\s\S]*sendMessage[\s\S]*\);|const onNew = useCallback\([\s\S]*sendMessage[\s\S]*\);/,
  );
  assert.match(
    chatRuntimeSource,
    /useExternalStoreRuntime<AssistantUiMessage>\(\{[\s\S]*messages,[\s\S]*convertMessage,[\s\S]*onNew,[\s\S]*\}\)/,
  );
});

test("assistant turn artifacts avoid fresh selector objects inside useAuiState", () => {
  assert.match(
    threadSource,
    /const rawTurnArtifacts = useAuiState\([\s\S]*s\.message\.metadata\.custom\.turnArtifacts[\s\S]*\);/,
  );
  assert.match(
    threadSource,
    /const turnArtifacts = useMemo\([\s\S]*readTurnArtifacts\(rawTurnArtifacts\)[\s\S]*\[rawTurnArtifacts\][\s\S]*\);|const turnArtifacts = React\.useMemo\([\s\S]*readTurnArtifacts\(rawTurnArtifacts\)[\s\S]*\[rawTurnArtifacts\][\s\S]*\);/,
  );
  assert.doesNotMatch(
    threadSource,
    /const turnArtifacts = useAuiState\(\(s\) =>\s*readTurnArtifacts\(/,
  );
});
