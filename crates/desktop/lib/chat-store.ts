import { create } from "zustand";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { agents, chat, jobRuntime, providers, sessions, threadRuntime } from "@/lib/tauri";
import type {
  JobDetailPayload,
  JobDetailTimelineItem,
  JobRuntimePoolState,
  JobStatusPayload,
  JobRuntimeSummary,
  MailboxMessageJobResultPayload,
  MailboxMessagePayload,
  RuntimeEventReason,
  RuntimeKind,
  JobRuntimePoolSnapshot,
  RuntimeStatus,
  ThreadEventEnvelope,
  ThreadRuntimeState,
  ThreadRuntimeSummary,
  ThreadSnapshotPayload,
} from "@/lib/types/chat";
import type { PlanItem } from "@/lib/types/plan";
import type { SessionSummary, ThreadSummary } from "@/lib/tauri";

export interface PendingToolCall {
  tool_call_id: string;
  tool_name: string;
  arguments_text: string;
  result?: unknown;
  is_error: boolean;
  status: "streaming" | "running" | "completed";
}

const toErrorMessage = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

const JOB_STATUS_DISPLAY_NAME_LIMIT = 80;
const JOB_STATUS_DESCRIPTION_LIMIT = 240;
const JOB_STATUS_PROMPT_LIMIT = 600;
const JOB_STATUS_MESSAGE_LIMIT = 1600;
const JOB_DETAIL_DISPLAY_NAME_LIMIT = 80;
const JOB_DETAIL_DESCRIPTION_LIMIT = 240;
const JOB_DETAIL_PROMPT_LIMIT = 1200;
const JOB_DETAIL_SUMMARY_LIMIT = 1600;
const JOB_DETAIL_RESULT_LIMIT = 12000;

const truncateDisplayText = (value: string, maxChars: number) => {
  const chars = Array.from(value);
  if (chars.length <= maxChars) return value;
  return `${chars.slice(0, maxChars).join("")}…`;
};

const truncateOptionalDisplayText = (
  value: string | null | undefined,
  maxChars: number,
) => {
  if (!value) return value ?? null;
  return truncateDisplayText(value, maxChars);
};

const normalizeJobStatusPayload = (
  payload: JobStatusPayload,
): JobStatusPayload => ({
  ...payload,
  prompt: truncateDisplayText(payload.prompt, JOB_STATUS_PROMPT_LIMIT),
  message: truncateOptionalDisplayText(
    payload.message,
    JOB_STATUS_MESSAGE_LIMIT,
  ),
  agent_display_name: truncateOptionalDisplayText(
    payload.agent_display_name,
    JOB_STATUS_DISPLAY_NAME_LIMIT,
  ),
  agent_description: truncateOptionalDisplayText(
    payload.agent_description,
    JOB_STATUS_DESCRIPTION_LIMIT,
  ),
});

const normalizeJobDetailPayload = (
  payload: JobDetailPayload,
): JobDetailPayload => ({
  ...payload,
  agent_display_name: truncateDisplayText(
    payload.agent_display_name,
    JOB_DETAIL_DISPLAY_NAME_LIMIT,
  ),
  agent_description: truncateOptionalDisplayText(
    payload.agent_description,
    JOB_DETAIL_DESCRIPTION_LIMIT,
  ),
  prompt: truncateDisplayText(payload.prompt, JOB_DETAIL_PROMPT_LIMIT),
  summary_text: truncateOptionalDisplayText(
    payload.summary_text,
    JOB_DETAIL_SUMMARY_LIMIT,
  ),
  result_text: truncateOptionalDisplayText(
    payload.result_text,
    JOB_DETAIL_RESULT_LIMIT,
  ),
});

function appendJobDetailTimelineEntry(
  jobDetails: Record<string, JobDetailPayload>,
  jobId: string,
  entry: JobDetailTimelineItem,
): Record<string, JobDetailPayload> {
  const current = jobDetails[jobId];
  if (!current) return jobDetails;
  return {
    ...jobDetails,
    [jobId]: normalizeJobDetailPayload({
      ...current,
      timeline: [...current.timeline, entry],
    }),
  };
}

function buildJobDetailTimelineEntry(
  kind: JobDetailTimelineItem["kind"],
  at: string,
  label: string,
  status: JobDetailTimelineItem["status"],
  reason?: RuntimeEventReason | null,
): JobDetailTimelineItem {
  return {
    kind,
    at,
    label,
    status,
    reason,
  };
}

function seedJobDetailFromDispatch(
  jobId: string,
  agentId: number,
  prompt: string,
  at: string,
  existing?: JobDetailPayload,
): JobDetailPayload {
  return normalizeJobDetailPayload({
    job_id: jobId,
    agent_id: agentId,
    agent_display_name: existing?.agent_display_name ?? `Agent ${agentId}`,
    agent_description: existing?.agent_description ?? "",
    prompt: existing?.prompt ?? prompt,
    status: "running",
    summary_text: existing?.summary_text ?? null,
    result_text: existing?.result_text ?? null,
    started_at: existing?.started_at ?? at,
    finished_at: existing?.finished_at ?? null,
    input_tokens: existing?.input_tokens ?? null,
    output_tokens: existing?.output_tokens ?? null,
    source_message_id: existing?.source_message_id ?? null,
    thread_id: existing?.thread_id ?? null,
    timeline: [
      ...(existing?.timeline ?? []),
      buildJobDetailTimelineEntry("dispatched", at, "已派发", "running"),
    ],
  });
}

function updateJobDetailFromResult(
  existing: JobDetailPayload | undefined,
  payload: Extract<ThreadEventEnvelope["payload"], { type: "job_result" }>,
  at: string,
  threadId: string,
  sourceMessageId?: string | null,
  resultText?: string | null,
): JobDetailPayload {
  const summaryText = existing?.summary_text ?? payload.message;
  return normalizeJobDetailPayload({
    job_id: payload.job_id,
    agent_id: payload.agent_id,
    agent_display_name: payload.agent_display_name,
    agent_description: payload.agent_description,
    prompt: existing?.prompt ?? "",
    status: payload.success ? "completed" : "failed",
    summary_text: summaryText,
    result_text: resultText ?? existing?.result_text ?? payload.message,
    started_at: existing?.started_at ?? null,
    finished_at: at,
    input_tokens: payload.input_tokens ?? existing?.input_tokens ?? null,
    output_tokens: payload.output_tokens ?? existing?.output_tokens ?? null,
    source_message_id: sourceMessageId ?? existing?.source_message_id ?? null,
    thread_id: threadId,
    timeline: [
      ...(existing?.timeline ?? []),
      buildJobDetailTimelineEntry(
        "result",
        at,
        payload.success ? "已完成" : "已失败",
        payload.success ? "completed" : "failed",
      ),
    ],
  });
}

function updateJobDetailFromMailboxResult(
  existing: JobDetailPayload | undefined,
  message: MailboxMessagePayload,
  result: MailboxMessageJobResultPayload,
): JobDetailPayload {
  const tokenUsage = result.token_usage ?? null;
  const summaryText = existing?.summary_text ?? message.summary ?? null;
  return normalizeJobDetailPayload({
    job_id: result.job_id,
    agent_id: result.agent_id,
    agent_display_name: result.agent_display_name,
    agent_description: result.agent_description,
    prompt: existing?.prompt ?? "",
    status: result.success ? "completed" : "failed",
    summary_text: summaryText,
    result_text: message.text,
    started_at: existing?.started_at ?? null,
    finished_at: message.timestamp,
    input_tokens: tokenUsage?.input_tokens ?? existing?.input_tokens ?? null,
    output_tokens: tokenUsage?.output_tokens ?? existing?.output_tokens ?? null,
    source_message_id: message.id,
    thread_id: message.to_thread_id,
    timeline: [
      ...(existing?.timeline ?? []),
      buildJobDetailTimelineEntry(
        "result",
        message.timestamp,
        result.success ? "收件箱收到完成结果" : "收件箱收到失败结果",
        result.success ? "completed" : "failed",
      ),
    ],
  });
}

type PendingAssistantState = NonNullable<ChatSessionState["pendingAssistant"]>;

const createPendingAssistant = (): PendingAssistantState => ({
  content: "",
  reasoning: "",
  toolCalls: [],
  plan: null,
  retry: null,
});

const ensurePendingAssistantSession = (
  session: ChatSessionState,
): ChatSessionState & { pendingAssistant: PendingAssistantState } => ({
  ...session,
  status: "running",
  pendingAssistant: session.pendingAssistant ?? createPendingAssistant(),
});

export interface RuntimeThreadState {
  threadId: string;
  kind: RuntimeKind;
  sessionId: string | null;
  jobId: string | null;
  status: RuntimeStatus;
  estimatedMemoryBytes: number;
  lastActiveAt: string | null;
  recoverable: boolean;
  lastReason: RuntimeEventReason | null;
  eventCount: number;
}

export interface ChatSessionState {
  sessionKey: string;
  sessionId: string;
  templateId: number;
  threadId: string;
  effectiveProviderId: number | null;
  effectiveModel: string | null;
  status: "idle" | "running" | "compacting" | "error";
  messages: ThreadSnapshotPayload["messages"];
  pendingUserMessage: string | null;
  pendingAssistant: {
    content: string;
    reasoning: string;
    toolCalls: PendingToolCall[];
    plan: PlanItem[] | null;
    retry: {
      attempt: number;
      maxRetries: number;
      error: string;
    } | null;
  } | null;
  jobStatuses: Record<string, JobStatusPayload>;
  jobDetails: Record<string, JobDetailPayload>;
  selectedJobDetailId: string | null;
  error: string | null;
  tokenCount: number;
  contextWindow: number | null;
}

export interface ChatStore {
  selectedTemplateId: number | null;
  selectedProviderPreferenceId: number | null;
  selectedModelOverride: string | null;
  activeSessionKey: string | null;
  errorMessage: string | null;
  sessionsByKey: Record<string, ChatSessionState>;
  templates: Awaited<ReturnType<typeof agents.list>>;
  providers: Awaited<ReturnType<typeof providers.list>>;
  sessionList: SessionSummary[];
  sessionListLoading: boolean;
  threadListBySessionId: Record<string, ThreadSummary[]>;
  threadListLoadingBySessionId: Record<string, boolean>;
  jobRuntimeSnapshot: JobRuntimePoolSnapshot | null;
  jobRuntimeSnapshotLoading: boolean;
  jobRuntimeError: string | null;
  runtimeThreads: RuntimeThreadState[];
  stoppingJobIds: Record<string, true>;
  _unlisten: UnlistenFn | null;

  initialize: () => Promise<void>;
  activateSession: (templateId: number) => Promise<void>;
  startNewSessionDraft: (templateId?: number | null) => void;
  switchToSession: (sessionId: string) => Promise<void>;
  switchToThread: (sessionId: string, threadId: string) => Promise<void>;
  loadSessionList: () => Promise<void>;
  loadThreads: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string) => Promise<void>;
  selectTemplate: (templateId: number) => void;
  selectModel: (providerId: number, model: string) => Promise<void>;
  selectProviderPreference: (providerId: number | null) => Promise<void>;
  selectModelOverride: (model: string | null) => Promise<void>;
  openJobDetails: (jobId: string) => void;
  closeJobDetails: () => void;
  sendMessage: (content: string) => Promise<void>;
  cancelTurn: () => Promise<void>;
  stopJob: (jobId: string) => Promise<void>;
  refreshRuntimeMonitor: () => Promise<void>;
  refreshSnapshot: (
    sessionKey: string,
    options?: { preserveError?: boolean },
  ) => Promise<void>;
  cleanup: () => void;
  _handleThreadEvent: (envelope: ThreadEventEnvelope) => void;
}

let threadEventListenerInitPromise: Promise<UnlistenFn> | null = null;
let runtimeMonitorRequestVersion = 0;

function clearStoppingJobId(
  stoppingJobIds: Record<string, true>,
  jobId: string,
): Record<string, true> {
  if (!(jobId in stoppingJobIds)) return stoppingJobIds;
  const nextStoppingJobIds = { ...stoppingJobIds };
  delete nextStoppingJobIds[jobId];
  return nextStoppingJobIds;
}

function getTemplateDraftSelection(
  templates: Awaited<ReturnType<typeof agents.list>>,
  templateId: number | null,
) {
  const agent = templates.find((entry) => entry.id === templateId);
  return {
    selectedTemplateId: templateId,
    selectedProviderPreferenceId: agent?.provider_id ?? null,
    selectedModelOverride: agent?.model_id ?? null,
  };
}

function mapRuntimeSummaryToThreadState(
  runtime: JobRuntimeSummary | ThreadRuntimeSummary,
  existing?: RuntimeThreadState,
): RuntimeThreadState {
  return {
    threadId: runtime.runtime.thread_id,
    kind: runtime.runtime.kind,
    sessionId: runtime.runtime.session_id,
    jobId: runtime.runtime.job_id,
    status: runtime.status,
    estimatedMemoryBytes: runtime.estimated_memory_bytes,
    lastActiveAt: runtime.last_active_at,
    recoverable: runtime.recoverable,
    lastReason: runtime.last_reason,
    eventCount: existing?.eventCount ?? 0,
  };
}

function sortRuntimeThreads(
  threads: RuntimeThreadState[],
): RuntimeThreadState[] {
  return threads.slice().sort((left, right) => {
    const leftTime = left.lastActiveAt ?? "";
    const rightTime = right.lastActiveAt ?? "";
    if (leftTime === rightTime) {
      return right.eventCount - left.eventCount;
    }
    return rightTime.localeCompare(leftTime);
  });
}

function touchRuntimeThread(
  threads: RuntimeThreadState[],
  update: Omit<
    RuntimeThreadState,
    | "estimatedMemoryBytes"
    | "lastActiveAt"
    | "recoverable"
    | "lastReason"
    | "eventCount"
  > & {
    estimatedMemoryBytes?: number;
    lastActiveAt?: string;
    recoverable?: boolean;
    reason?: RuntimeEventReason | null;
  },
  fallbackSnapshot: JobRuntimePoolSnapshot | null,
): RuntimeThreadState[] {
  const now = update.lastActiveAt ?? new Date().toISOString();
  const existing = threads.find((entry) => entry.threadId === update.threadId);
  const nextEntry: RuntimeThreadState = {
    threadId: update.threadId,
    kind: update.kind,
    sessionId: update.sessionId,
    jobId: update.jobId,
    status: update.status,
    estimatedMemoryBytes:
      update.estimatedMemoryBytes ??
      existing?.estimatedMemoryBytes ??
      fallbackSnapshot?.avg_thread_memory_bytes ??
      0,
    lastActiveAt: now,
    recoverable: update.recoverable ?? existing?.recoverable ?? true,
    lastReason: update.reason ?? existing?.lastReason ?? null,
    eventCount: (existing?.eventCount ?? 0) + 1,
  };

  const withoutCurrent = threads.filter(
    (entry) => entry.threadId !== update.threadId,
  );
  return sortRuntimeThreads([nextEntry, ...withoutCurrent]);
}

function mergeRuntimeThreads(
  jobRuntimeState: JobRuntimePoolState,
  threadRuntimeState: ThreadRuntimeState,
  existingThreads: RuntimeThreadState[],
): RuntimeThreadState[] {
  const runtimes = [
    ...threadRuntimeState.runtimes,
    ...jobRuntimeState.runtimes,
  ];

  return sortRuntimeThreads(
    runtimes.map((runtime) =>
      mapRuntimeSummaryToThreadState(
        runtime,
        existingThreads.find(
          (thread) => thread.threadId === runtime.runtime.thread_id,
        ),
      ),
    ),
  );
}

function findSessionKeyForEnvelope(
  sessionsByKey: Record<string, ChatSessionState>,
  envelope: ThreadEventEnvelope,
): string | null {
  const exactMatch =
    Object.keys(sessionsByKey).find(
      (key) =>
        sessionsByKey[key].threadId === envelope.thread_id &&
        sessionsByKey[key].sessionId === envelope.session_id,
    ) ?? null;
  if (exactMatch) return exactMatch;

  const jobId =
    envelope.payload.type === "thread_bound_to_job"
      ? envelope.payload.job_id
      : envelope.payload.type === "job_runtime_queued" ||
          envelope.payload.type === "job_runtime_started" ||
          envelope.payload.type === "job_runtime_cooling" ||
          envelope.payload.type === "job_runtime_evicted"
        ? envelope.payload.runtime.job_id
        : null;

  if (!jobId) return null;

  return (
    Object.keys(sessionsByKey).find((key) => {
      const session = sessionsByKey[key];
      return (
        session.sessionId === envelope.session_id && !!session.jobDetails[jobId]
      );
    }) ?? null
  );
}

function updateSessionJobDetailTimeline(
  sessionsByKey: Record<string, ChatSessionState>,
  sessionKey: string | null,
  jobId: string | null,
  entry: JobDetailTimelineItem,
): Record<string, ChatSessionState> | null {
  if (!sessionKey || !jobId) return null;

  const session = sessionsByKey[sessionKey];
  if (!session?.jobDetails[jobId]) return null;

  return {
    ...sessionsByKey,
    [sessionKey]: {
      ...session,
      jobDetails: appendJobDetailTimelineEntry(
        session.jobDetails,
        jobId,
        entry,
      ),
    },
  };
}

export const useChatStore = create<ChatStore>((set, get) => ({
  selectedTemplateId: null,
  selectedProviderPreferenceId: null,
  selectedModelOverride: null,
  activeSessionKey: null,
  errorMessage: null,
  sessionsByKey: {},
  templates: [],
  providers: [],
  sessionList: [],
  sessionListLoading: false,
  threadListBySessionId: {},
  threadListLoadingBySessionId: {},
  jobRuntimeSnapshot: null,
  jobRuntimeSnapshotLoading: false,
  jobRuntimeError: null,
  runtimeThreads: [],
  stoppingJobIds: {},
  _unlisten: null,

  async initialize() {
    try {
      if (!get()._unlisten) {
        threadEventListenerInitPromise ??= listen<ThreadEventEnvelope>(
          "thread:event",
          (event) => {
            get()._handleThreadEvent(event.payload);
          },
        )
          .then((unlisten) => {
            set((state) => (state._unlisten ? {} : { _unlisten: unlisten }));
            return unlisten;
          })
          .finally(() => {
            threadEventListenerInitPromise = null;
          });

        await threadEventListenerInitPromise;
      }

      const [templateList, providerList] = await Promise.all([
        agents.list(),
        providers.list(),
      ]);
      set((state) => ({
        templates: templateList,
        providers: providerList,
        errorMessage: null,
        selectedTemplateId:
          state.selectedTemplateId ?? templateList[0]?.id ?? null,
      }));

      if (templateList.length === 0) {
        set({ errorMessage: "当前没有可用的 Agent 模板。" });
      }
      void get().refreshRuntimeMonitor();
      // NOTE: Do NOT auto-create a session here.
      // Session/thread materialization happens only on first send.
    } catch (error) {
      set({ errorMessage: toErrorMessage(error) });
    }
  },

  async activateSession(templateId: number) {
    const state = get();

    try {
      const session = await chat.createChatSession(
        templateId,
        state.selectedProviderPreferenceId,
        state.selectedModelOverride,
      );
      const snapshot = await chat.getThreadSnapshot(
        session.session_id,
        session.thread_id,
      );

      const newSessionState: ChatSessionState = {
        sessionKey: session.session_key,
        sessionId: session.session_id,
        templateId: session.template_id,
        threadId: session.thread_id,
        effectiveProviderId: session.effective_provider_id,
        effectiveModel: session.effective_model,
        status: "idle",
        messages: snapshot.messages,
        pendingUserMessage: null,
        pendingAssistant: null,
        jobStatuses: {},
        jobDetails: {},
        selectedJobDetailId: null,
        error: null,
        tokenCount: 0,
        contextWindow: null,
      };

      set((state) => ({
        activeSessionKey: session.session_id,
        selectedTemplateId: templateId,
        errorMessage: null,
        threadListBySessionId: {
          ...state.threadListBySessionId,
          [session.session_id]: [
            {
              thread_id: session.thread_id,
              title: null,
              turn_count: snapshot.turn_count,
              token_count: snapshot.token_count,
              updated_at: new Date().toISOString(),
            },
          ],
        },
        sessionsByKey: {
          ...state.sessionsByKey,
          [session.session_id]: newSessionState,
        },
      }));
    } catch (error) {
      set({
        selectedTemplateId: templateId,
        errorMessage: toErrorMessage(error),
      });
      throw error;
    }
  },

  startNewSessionDraft: (templateId?: number | null) => {
    set((state) => {
      const nextDraftSelection =
        templateId == null
          ? {}
          : getTemplateDraftSelection(state.templates, templateId);

      return {
        ...nextDraftSelection,
        activeSessionKey: null,
        errorMessage: null,
      };
    });
  },

  async switchToSession(sessionId: string) {
    await get().loadThreads(sessionId);

    const threadList = get().threadListBySessionId[sessionId] ?? [];
    if (threadList.length === 0) {
      const error = new Error("No threads in session");
      set({ errorMessage: error.message });
      throw error;
    }

    await get().switchToThread(sessionId, threadList[0].thread_id);
  },

  async switchToThread(sessionId: string, threadId: string) {
    const state = get();
    const existingSession = state.sessionsByKey[sessionId];
    if (existingSession?.threadId === threadId) {
      set((currentState) => ({
        activeSessionKey: sessionId,
        errorMessage: null,
        sessionsByKey: {
          ...currentState.sessionsByKey,
          [sessionId]: {
            ...existingSession,
            selectedJobDetailId: null,
          },
        },
      }));
      return;
    }

    try {
      const activated = await chat.activateExistingThread(sessionId, threadId);
      const snapshot = await chat.getThreadSnapshot(sessionId, threadId);

      const nextSessionState: ChatSessionState = {
        sessionKey: activated.session_key,
        sessionId: sessionId,
        templateId: activated.template_id,
        threadId: threadId,
        effectiveProviderId: activated.effective_provider_id,
        effectiveModel: activated.effective_model,
        status: "idle",
        messages: snapshot.messages,
        pendingUserMessage: null,
        pendingAssistant: null,
        jobStatuses: {},
        jobDetails: {},
        selectedJobDetailId: null,
        error: null,
        tokenCount: snapshot.token_count,
        contextWindow: null,
      };

      set((currentState) => ({
        activeSessionKey: sessionId,
        errorMessage: null,
        sessionsByKey: {
          ...currentState.sessionsByKey,
          [sessionId]: nextSessionState,
        },
      }));
    } catch (error) {
      set({ errorMessage: toErrorMessage(error) });
      throw error;
    }
  },

  async loadSessionList() {
    set({ sessionListLoading: true });
    try {
      const list = await sessions.list();
      set({ sessionList: list, sessionListLoading: false });
    } catch (error) {
      set({ sessionListLoading: false, errorMessage: toErrorMessage(error) });
    }
  },

  async loadThreads(sessionId: string) {
    set((state) => ({
      threadListLoadingBySessionId: {
        ...state.threadListLoadingBySessionId,
        [sessionId]: true,
      },
    }));

    try {
      const threadList = await chat.listThreads(sessionId);
      set((state) => ({
        errorMessage: null,
        threadListBySessionId: {
          ...state.threadListBySessionId,
          [sessionId]: threadList,
        },
        threadListLoadingBySessionId: {
          ...state.threadListLoadingBySessionId,
          [sessionId]: false,
        },
      }));
    } catch (error) {
      set((state) => ({
        errorMessage: toErrorMessage(error),
        threadListLoadingBySessionId: {
          ...state.threadListLoadingBySessionId,
          [sessionId]: false,
        },
      }));
    }
  },

  async deleteSession(sessionId: string) {
    try {
      await sessions.delete(sessionId);
      set((state) => {
        const newSessionsByKey = { ...state.sessionsByKey };
        const newThreadLists = { ...state.threadListBySessionId };
        const newThreadLoading = { ...state.threadListLoadingBySessionId };
        delete newSessionsByKey[sessionId];
        delete newThreadLists[sessionId];
        delete newThreadLoading[sessionId];
        return {
          sessionList: state.sessionList.filter((s) => s.id !== sessionId),
          sessionsByKey: newSessionsByKey,
          threadListBySessionId: newThreadLists,
          threadListLoadingBySessionId: newThreadLoading,
          activeSessionKey:
            state.activeSessionKey === sessionId
              ? null
              : state.activeSessionKey,
        };
      });
    } catch (error) {
      set({ errorMessage: toErrorMessage(error) });
    }
  },

  selectTemplate(templateId: number) {
    set({
      ...getTemplateDraftSelection(get().templates, templateId),
      errorMessage: null,
    });
  },

  async selectModel(providerId: number, model: string) {
    const state = get();
    const provider = state.providers.find((entry) => entry.id === providerId);
    if (!provider) {
      const errorMessage = `Provider not found: ${providerId}`;
      set({ errorMessage });
      throw new Error(errorMessage);
    }

    const normalizedOverride = model === provider.default_model ? null : model;
    const activeSessionKey = state.activeSessionKey;
    const activeSession = activeSessionKey
      ? (state.sessionsByKey[activeSessionKey] ?? null)
      : null;

    if (!activeSession || !activeSessionKey) {
      set({
        selectedProviderPreferenceId: providerId,
        selectedModelOverride: normalizedOverride,
        errorMessage: null,
      });
      return;
    }

    try {
      const updated = await chat.updateThreadModel(
        activeSession.sessionId,
        activeSession.threadId,
        providerId,
        model,
      );

      set((currentState) => ({
        selectedProviderPreferenceId: providerId,
        selectedModelOverride: normalizedOverride,
        errorMessage: null,
        sessionsByKey: {
          ...currentState.sessionsByKey,
          [activeSessionKey]: {
            ...currentState.sessionsByKey[activeSessionKey],
            effectiveProviderId: updated.effective_provider_id,
            effectiveModel: updated.effective_model,
            error: null,
          },
        },
      }));
    } catch (error) {
      const errorMessage = toErrorMessage(error);
      set({ errorMessage });
      throw error;
    }
  },

  async selectProviderPreference(providerId: number | null) {
    set({ selectedProviderPreferenceId: providerId, errorMessage: null });
  },

  async selectModelOverride(model: string | null) {
    set({ selectedModelOverride: model, errorMessage: null });
  },

  openJobDetails(jobId: string) {
    const sessionKey = get().activeSessionKey;
    if (!sessionKey) return;

    set((state) => {
      const session = state.sessionsByKey[sessionKey];
      if (!session) return {};

      return {
        sessionsByKey: {
          ...state.sessionsByKey,
          [sessionKey]: {
            ...session,
            selectedJobDetailId: jobId,
          },
        },
      };
    });
  },

  closeJobDetails() {
    const sessionKey = get().activeSessionKey;
    if (!sessionKey) return;

    set((state) => {
      const session = state.sessionsByKey[sessionKey];
      if (!session) return {};
      return {
        sessionsByKey: {
          ...state.sessionsByKey,
          [sessionKey]: {
            ...session,
            selectedJobDetailId: null,
          },
        },
      };
    });
  },

  async sendMessage(content: string) {
    const trimmedContent = content.trim();
    if (!trimmedContent) return;

    let state = get();
    if (!state.activeSessionKey) {
      const fallbackTemplateId =
        state.selectedTemplateId ?? state.templates[0]?.id ?? null;
      if (!fallbackTemplateId) {
        set({ errorMessage: "当前没有可用的聊天会话。" });
        return;
      }

      try {
        await get().activateSession(fallbackTemplateId);
      } catch {
        return;
      }

      state = get();
    }

    if (!state.activeSessionKey) {
      set({ errorMessage: "当前会话尚未准备好，请稍后重试。" });
      return;
    }

    const session = state.sessionsByKey[state.activeSessionKey];
    if (!session) {
      set({ errorMessage: "当前会话尚未准备好，请稍后重试。" });
      return;
    }

    set((state) => ({
      errorMessage: null,
      sessionsByKey: {
        ...state.sessionsByKey,
        [state.activeSessionKey!]: {
          ...session,
          status: "running",
          pendingUserMessage: trimmedContent,
          pendingAssistant: {
            content: "",
            reasoning: "",
            toolCalls: [],
            plan: null,
            retry: null,
          },
          error: null,
        },
      },
    }));

    try {
      await chat.sendMessage(
        session.sessionId,
        session.threadId,
        trimmedContent,
      );
    } catch (error) {
      const errorMessage = toErrorMessage(error);
      set((store) => ({
        errorMessage,
        sessionsByKey: {
          ...store.sessionsByKey,
          [state.activeSessionKey!]: {
            ...store.sessionsByKey[state.activeSessionKey!],
            status: "error",
            pendingUserMessage: null,
            pendingAssistant: null,
            error: errorMessage,
          },
        },
      }));
    }
  },

  async refreshRuntimeMonitor() {
    const requestVersion = ++runtimeMonitorRequestVersion;
    set((state) => ({
      jobRuntimeSnapshotLoading: true,
      jobRuntimeError: null,
      jobRuntimeSnapshot: state.jobRuntimeSnapshot,
    }));

    try {
      const [jobRuntimeState, threadRuntimeState] = await Promise.all([
        jobRuntime.getState(),
        threadRuntime.getState(),
      ]);
      set((state) => ({
        ...(requestVersion === runtimeMonitorRequestVersion
          ? {
              jobRuntimeSnapshot: jobRuntimeState.snapshot,
              jobRuntimeSnapshotLoading: false,
              jobRuntimeError: null,
              runtimeThreads: mergeRuntimeThreads(
                jobRuntimeState,
                threadRuntimeState,
                state.runtimeThreads,
              ),
            }
          : {}),
      }));
    } catch (error) {
      set(() =>
        requestVersion === runtimeMonitorRequestVersion
          ? {
              jobRuntimeSnapshotLoading: false,
              jobRuntimeError: toErrorMessage(error),
            }
          : {},
      );
    }
  },

  async cancelTurn() {
    const state = get();
    const sessionKey = state.activeSessionKey;
    if (!sessionKey) return;
    const session = state.sessionsByKey[sessionKey];
    if (!session || session.status !== "running") return;

    try {
      await chat.cancelTurn(session.sessionId, session.threadId);
    } catch (error) {
      console.error("取消 turn 失败:", error);
    }
  },

  async stopJob(jobId: string) {
    if (!jobId) return;
    if (get().stoppingJobIds[jobId]) return;

    set((state) => ({
      errorMessage: null,
      stoppingJobIds: {
        ...state.stoppingJobIds,
        [jobId]: true,
      },
    }));

    try {
      await chat.stopJob(jobId);
    } catch (error) {
      const errorMessage = toErrorMessage(error);
      set((state) => ({
        errorMessage: errorMessage,
        stoppingJobIds: clearStoppingJobId(state.stoppingJobIds, jobId),
      }));
      throw error;
    }
  },

  async refreshSnapshot(
    sessionKey: string,
    options?: { preserveError?: boolean },
  ) {
    const session = get().sessionsByKey[sessionKey];
    if (!session) return;

    try {
      const snapshot = await chat.getThreadSnapshot(
        session.sessionId,
        session.threadId,
      );
      set((state) => ({
        errorMessage: options?.preserveError ? state.errorMessage : null,
        sessionsByKey: {
          ...state.sessionsByKey,
          [sessionKey]: {
            ...state.sessionsByKey[sessionKey],
            messages: snapshot.messages,
            pendingUserMessage: null,
            pendingAssistant: null,
            status: options?.preserveError ? "error" : "idle",
            error: options?.preserveError
              ? state.sessionsByKey[sessionKey].error
              : null,
            tokenCount: snapshot.token_count,
            jobStatuses: state.sessionsByKey[sessionKey].jobStatuses,
            jobDetails: state.sessionsByKey[sessionKey].jobDetails,
            selectedJobDetailId:
              state.sessionsByKey[sessionKey].selectedJobDetailId,
          },
        },
      }));
    } catch (error) {
      const errorMessage = toErrorMessage(error);
      set((state) => ({
        errorMessage,
        sessionsByKey: {
          ...state.sessionsByKey,
          [sessionKey]: {
            ...state.sessionsByKey[sessionKey],
            pendingUserMessage: null,
            pendingAssistant: null,
            status: "error",
            error: errorMessage,
          },
        },
      }));
    }
  },

  cleanup() {
    const unlisten = get()._unlisten;
    if (unlisten) {
      unlisten();
      set({ _unlisten: null });
    }
    threadEventListenerInitPromise = null;
  },

  _handleThreadEvent(envelope: ThreadEventEnvelope) {
    const state = get();
    const sessionKey = findSessionKeyForEnvelope(state.sessionsByKey, envelope);
    const poolHandled = (() => {
      const { payload } = envelope;

      switch (payload.type) {
        case "thread_bound_to_job":
        case "job_runtime_queued":
        case "job_runtime_started":
        case "job_runtime_cooling":
        case "job_runtime_evicted":
        case "job_runtime_metrics_updated":
          return true;
        default:
          return false;
      }
    })();

    if (poolHandled) {
      const now = new Date().toISOString();
      set((state) => {
        const { payload } = envelope;

        switch (payload.type) {
          case "thread_bound_to_job": {
            const nextSessionsByKey = updateSessionJobDetailTimeline(
              state.sessionsByKey,
              sessionKey,
              payload.job_id,
              buildJobDetailTimelineEntry(
                "dispatched",
                now,
                "线程已绑定任务",
                "running",
              ),
            );
            return {
              runtimeThreads: touchRuntimeThread(
                state.runtimeThreads,
                {
                  threadId: envelope.thread_id,
                  kind: "job",
                  sessionId: null,
                  jobId: payload.job_id,
                  status: "queued",
                  reason: null,
                  lastActiveAt: now,
                },
                state.jobRuntimeSnapshot,
              ),
              ...(nextSessionsByKey
                ? { sessionsByKey: nextSessionsByKey }
                : {}),
            };
          }
          case "job_runtime_queued": {
            const nextSessionsByKey = updateSessionJobDetailTimeline(
              state.sessionsByKey,
              sessionKey,
              payload.runtime.job_id,
              buildJobDetailTimelineEntry("queued", now, "排队中", "running"),
            );
            return {
              runtimeThreads: touchRuntimeThread(
                state.runtimeThreads,
                {
                  threadId: envelope.thread_id,
                  kind: payload.runtime.kind,
                  sessionId: payload.runtime.session_id,
                  jobId: payload.runtime.job_id,
                  status: "queued",
                  reason: null,
                  lastActiveAt: now,
                },
                state.jobRuntimeSnapshot,
              ),
              ...(nextSessionsByKey
                ? { sessionsByKey: nextSessionsByKey }
                : {}),
            };
          }
          case "job_runtime_started": {
            const nextSessionsByKey = updateSessionJobDetailTimeline(
              state.sessionsByKey,
              sessionKey,
              payload.runtime.job_id,
              buildJobDetailTimelineEntry("started", now, "运行中", "running"),
            );
            return {
              runtimeThreads: touchRuntimeThread(
                state.runtimeThreads,
                {
                  threadId: envelope.thread_id,
                  kind: payload.runtime.kind,
                  sessionId: payload.runtime.session_id,
                  jobId: payload.runtime.job_id,
                  status: "running",
                  reason: null,
                  lastActiveAt: now,
                },
                state.jobRuntimeSnapshot,
              ),
              ...(nextSessionsByKey
                ? { sessionsByKey: nextSessionsByKey }
                : {}),
            };
          }
          case "job_runtime_cooling": {
            const nextSessionsByKey = updateSessionJobDetailTimeline(
              state.sessionsByKey,
              sessionKey,
              payload.runtime.job_id,
              buildJobDetailTimelineEntry("cooling", now, "冷却中", "running"),
            );
            return {
              runtimeThreads: touchRuntimeThread(
                state.runtimeThreads,
                {
                  threadId: envelope.thread_id,
                  kind: payload.runtime.kind,
                  sessionId: payload.runtime.session_id,
                  jobId: payload.runtime.job_id,
                  status: "cooling",
                  reason: null,
                  lastActiveAt: now,
                },
                state.jobRuntimeSnapshot,
              ),
              ...(nextSessionsByKey
                ? { sessionsByKey: nextSessionsByKey }
                : {}),
            };
          }
          case "job_runtime_evicted": {
            const nextSessionsByKey = updateSessionJobDetailTimeline(
              state.sessionsByKey,
              sessionKey,
              payload.runtime.job_id,
              buildJobDetailTimelineEntry(
                "evicted",
                now,
                "已驱逐",
                "failed",
                payload.reason,
              ),
            );
            return {
              runtimeThreads: touchRuntimeThread(
                state.runtimeThreads,
                {
                  threadId: envelope.thread_id,
                  kind: payload.runtime.kind,
                  sessionId: payload.runtime.session_id,
                  jobId: payload.runtime.job_id,
                  status: "evicted",
                  reason: payload.reason,
                  lastActiveAt: now,
                },
                state.jobRuntimeSnapshot,
              ),
              ...(nextSessionsByKey
                ? { sessionsByKey: nextSessionsByKey }
                : {}),
            };
          }
          case "job_runtime_metrics_updated":
            return {
              jobRuntimeSnapshotLoading: false,
              jobRuntimeError: null,
              jobRuntimeSnapshot: payload.snapshot,
              runtimeThreads: state.runtimeThreads.map((thread) => ({
                ...thread,
                estimatedMemoryBytes:
                  thread.estimatedMemoryBytes > 0
                    ? thread.estimatedMemoryBytes
                    : payload.snapshot.avg_thread_memory_bytes,
              })),
            };
          default:
            return {};
        }
      });
    }

    if (!sessionKey) return;

    const { payload } = envelope;

    switch (payload.type) {
      case "retry_attempt":
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session) return {};
          const sessionWithPending = ensurePendingAssistantSession(session);
          return {
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...sessionWithPending,
                pendingAssistant: {
                  ...sessionWithPending.pendingAssistant,
                  retry: {
                    attempt: payload.attempt,
                    maxRetries: payload.max_retries,
                    error: payload.error,
                  },
                },
              },
            },
          };
        });
        break;

      case "reasoning_delta":
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session) return {};
          const sessionWithPending = ensurePendingAssistantSession(session);
          return {
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...sessionWithPending,
                pendingAssistant: {
                  ...sessionWithPending.pendingAssistant,
                  reasoning:
                    sessionWithPending.pendingAssistant.reasoning + payload.delta,
                  retry: null,
                },
              },
            },
          };
        });
        break;

      case "content_delta":
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session) return {};
          const sessionWithPending = ensurePendingAssistantSession(session);
          return {
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...sessionWithPending,
                pendingAssistant: {
                  ...sessionWithPending.pendingAssistant,
                  content:
                    sessionWithPending.pendingAssistant.content + payload.delta,
                  retry: null,
                },
              },
            },
          };
        });
        break;

      case "tool_call_delta": {
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session) return {};
          const sessionWithPending = ensurePendingAssistantSession(session);
          const toolCalls = [...sessionWithPending.pendingAssistant.toolCalls];
          while (toolCalls.length <= payload.index) {
            toolCalls.push({
              tool_call_id: "",
              tool_name: "",
              arguments_text: "",
              is_error: false,
              status: "streaming",
            });
          }
          const tc = { ...toolCalls[payload.index] };
          if (payload.id !== undefined && payload.id !== null) {
            tc.tool_call_id = payload.id;
          }
          if (payload.name !== undefined && payload.name !== null) {
            tc.tool_name = payload.name;
          }
          if (
            payload.arguments_delta !== undefined &&
            payload.arguments_delta !== null
          ) {
            tc.arguments_text += payload.arguments_delta;
          }
          toolCalls[payload.index] = tc;
          return {
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...sessionWithPending,
                pendingAssistant: {
                  ...sessionWithPending.pendingAssistant,
                  toolCalls,
                  retry: null,
                },
              },
            },
          };
        });
        break;
      }

      case "tool_started": {
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session) return {};
          const sessionWithPending = ensurePendingAssistantSession(session);
          const existingIndex = sessionWithPending.pendingAssistant.toolCalls.findIndex(
            (tc) => tc.tool_call_id === payload.tool_call_id,
          );
          const toolCalls = [...sessionWithPending.pendingAssistant.toolCalls];
          if (existingIndex >= 0) {
            toolCalls[existingIndex] = {
              ...toolCalls[existingIndex],
              status: "running",
            };
          } else {
            toolCalls.push({
              tool_call_id: payload.tool_call_id,
              tool_name: payload.tool_name,
              arguments_text: JSON.stringify(payload.arguments ?? {}, null, 2),
              is_error: false,
              status: "running",
            });
          }
          const updates: Partial<ChatSessionState> = {
            pendingAssistant: {
              ...sessionWithPending.pendingAssistant,
              toolCalls,
              retry: null,
            },
          };
          if (payload.tool_name === "update_plan" && payload.arguments) {
            const args = payload.arguments as { plan?: PlanItem[] };
            if (Array.isArray(args.plan)) {
              updates.pendingAssistant = {
                ...updates.pendingAssistant!,
                plan: args.plan,
              };
            }
          }
          return {
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...sessionWithPending,
                ...updates,
              },
            },
          };
        });
        break;
      }

      case "tool_completed": {
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session) return {};
          const sessionWithPending = ensurePendingAssistantSession(session);
          const existingIndex = sessionWithPending.pendingAssistant.toolCalls.findIndex(
            (tc) => tc.tool_call_id === payload.tool_call_id,
          );
          if (existingIndex < 0) return {};
          const toolCalls = [...sessionWithPending.pendingAssistant.toolCalls];
          toolCalls[existingIndex] = {
            ...toolCalls[existingIndex],
            tool_name: payload.tool_name,
            result: payload.result,
            is_error: payload.is_error,
            status: "completed",
          };
          const updates: Partial<ChatSessionState> = {
            pendingAssistant: {
              ...sessionWithPending.pendingAssistant,
              toolCalls,
              retry: null,
            },
          };
          if (payload.tool_name === "update_plan") {
            const result = payload.result as { plan?: PlanItem[] } | null;
            updates.pendingAssistant = {
              ...updates.pendingAssistant!,
              plan: payload.is_error
                ? null
                : Array.isArray(result?.plan)
                  ? result.plan
                  : null,
            };
          }
          return {
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...sessionWithPending,
                ...updates,
              },
            },
          };
        });
        break;
      }

      case "job_dispatched":
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session) return {};
          const now = new Date().toISOString();
          const nextJobDetail = seedJobDetailFromDispatch(
            payload.job_id,
            payload.agent_id,
            payload.prompt,
            now,
            session.jobDetails[payload.job_id],
          );
          const nextJobStatus = normalizeJobStatusPayload({
            job_id: payload.job_id,
            agent_id: payload.agent_id,
            prompt: payload.prompt,
            status: "running",
            message: null,
            agent_display_name:
              session.jobStatuses[payload.job_id]?.agent_display_name ?? null,
            agent_description:
              session.jobStatuses[payload.job_id]?.agent_description ?? null,
          });
          return {
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...session,
                jobDetails: {
                  ...session.jobDetails,
                  [payload.job_id]: nextJobDetail,
                },
                jobStatuses: {
                  ...session.jobStatuses,
                  [payload.job_id]: nextJobStatus,
                },
              },
            },
          };
        });
        break;

      case "job_result":
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          const stoppingJobIds = clearStoppingJobId(
            state.stoppingJobIds,
            payload.job_id,
          );
          if (!session) {
            return { stoppingJobIds };
          }
          const existing = session.jobStatuses[payload.job_id];
          const nextJobDetail = updateJobDetailFromResult(
            session.jobDetails[payload.job_id],
            payload,
            new Date().toISOString(),
            session.threadId,
          );
          const nextJobStatus = normalizeJobStatusPayload({
            job_id: payload.job_id,
            agent_id: payload.agent_id,
            prompt: existing?.prompt ?? "",
            status: payload.success ? "completed" : "failed",
            message: payload.message,
            agent_display_name: payload.agent_display_name,
            agent_description: payload.agent_description,
          });
          return {
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...session,
                jobDetails: {
                  ...session.jobDetails,
                  [payload.job_id]: nextJobDetail,
                },
                jobStatuses: {
                  ...session.jobStatuses,
                  [payload.job_id]: nextJobStatus,
                },
              },
            },
            stoppingJobIds,
          };
        });
        break;

      case "mailbox_message_queued":
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session) return {};
          const { message } = payload;
          if (message.message_type.type === "job_result") {
            const nextJobDetail = updateJobDetailFromMailboxResult(
              session.jobDetails[message.message_type.job_id],
              message,
              message.message_type,
            );
            return {
              sessionsByKey: {
                ...state.sessionsByKey,
                [sessionKey]: {
                  ...session,
                  jobDetails: {
                    ...session.jobDetails,
                    [message.message_type.job_id]: nextJobDetail,
                  },
                },
              },
            };
          }
          return {};
        });
        break;

      case "turn_completed":
        set((state) => ({
          sessionsByKey: {
            ...state.sessionsByKey,
            [sessionKey]: {
              ...state.sessionsByKey[sessionKey],
              tokenCount: payload.total_tokens,
              pendingAssistant: state.sessionsByKey[sessionKey]
                ?.pendingAssistant
                ? {
                    ...state.sessionsByKey[sessionKey].pendingAssistant!,
                    retry: null,
                  }
                : null,
            },
          },
        }));
        break;

      case "llm_usage":
        set((state) => ({
          sessionsByKey: {
            ...state.sessionsByKey,
            [sessionKey]: {
              ...state.sessionsByKey[sessionKey],
              tokenCount: payload.total_tokens,
            },
          },
        }));
        break;

      case "compacted":
        set((state) => ({
          sessionsByKey: {
            ...state.sessionsByKey,
            [sessionKey]: {
              ...state.sessionsByKey[sessionKey],
              tokenCount: payload.new_token_count,
            },
          },
        }));
        break;

      case "compaction_started":
        set((state) => ({
          sessionsByKey: {
            ...state.sessionsByKey,
            [sessionKey]: {
              ...state.sessionsByKey[sessionKey],
              status: "compacting",
            },
          },
        }));
        break;

      case "compaction_finished":
        set((state) => ({
          sessionsByKey: {
            ...state.sessionsByKey,
            [sessionKey]: {
              ...state.sessionsByKey[sessionKey],
              status: "running",
            },
          },
        }));
        break;

      case "compaction_failed":
        set((state) => ({
          sessionsByKey: {
            ...state.sessionsByKey,
            [sessionKey]: {
              ...state.sessionsByKey[sessionKey],
              status: "running",
            },
          },
        }));
        break;

      case "turn_failed":
        set((store) => ({
          errorMessage: payload.error,
          sessionsByKey: {
            ...store.sessionsByKey,
            [sessionKey]: {
              ...store.sessionsByKey[sessionKey],
              status: "error",
              pendingUserMessage: null,
              pendingAssistant: null,
              error: payload.error,
            },
          },
        }));
        void get().refreshSnapshot(sessionKey, { preserveError: true });
        break;

      case "idle":
        if (get().sessionsByKey[sessionKey]?.status !== "error") {
          void get().refreshSnapshot(sessionKey);
        }
        break;
    }
  },
}));
