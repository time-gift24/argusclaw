import { create } from "zustand";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { agents, chat, providers, sessions, threadPool } from "@/lib/tauri";
import type {
  ApprovalRequestPayload,
  JobStatusPayload,
  ThreadEventEnvelope,
  ThreadPoolEventReason,
  ThreadPoolRuntimeKind,
  ThreadPoolRuntimeSummary,
  ThreadPoolSnapshot,
  ThreadSnapshotPayload,
  ThreadRuntimeStatus,
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

export interface ThreadPoolThreadState {
  threadId: string;
  kind: ThreadPoolRuntimeKind;
  sessionId: string | null;
  jobId: string | null;
  status: ThreadRuntimeStatus;
  estimatedMemoryBytes: number;
  lastActiveAt: string | null;
  recoverable: boolean;
  lastReason: ThreadPoolEventReason | null;
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
  pendingAssistant: {
    content: string;
    reasoning: string;
    toolCalls: PendingToolCall[];
    plan: PlanItem[] | null;
  } | null;
  pendingApprovalRequest: {
    id: string;
    tool_name: string;
    action: string;
    risk_level: ApprovalRequestPayload["risk_level"];
    requested_at: string;
    timeout_secs: number;
  } | null;
  jobStatuses: Record<string, JobStatusPayload>;
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
  threadPoolSnapshot: ThreadPoolSnapshot | null;
  threadPoolSnapshotLoading: boolean;
  threadPoolError: string | null;
  threadPoolThreads: ThreadPoolThreadState[];
  stoppingJobIds: Record<string, true>;
  _unlisten: UnlistenFn | null;

  initialize: () => Promise<void>;
  activateSession: (templateId: number) => Promise<void>;
  switchToSession: (sessionId: string) => Promise<void>;
  switchToThread: (sessionId: string, threadId: string) => Promise<void>;
  loadSessionList: () => Promise<void>;
  loadThreads: (sessionId: string) => Promise<void>;
  deleteSession: (sessionId: string) => Promise<void>;
  selectTemplate: (templateId: number) => void;
  selectModel: (providerId: number, model: string) => Promise<void>;
  selectProviderPreference: (providerId: number | null) => Promise<void>;
  selectModelOverride: (model: string | null) => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  cancelTurn: () => Promise<void>;
  stopJob: (jobId: string) => Promise<void>;
  refreshThreadPoolSnapshot: () => Promise<void>;
  refreshSnapshot: (
    sessionKey: string,
    options?: { preserveError?: boolean },
  ) => Promise<void>;
  cleanup: () => void;
  _handleThreadEvent: (envelope: ThreadEventEnvelope) => void;
}

let threadEventListenerInitPromise: Promise<UnlistenFn> | null = null;
let threadPoolSnapshotRequestVersion = 0;

function clearStoppingJobId(
  stoppingJobIds: Record<string, true>,
  jobId: string,
): Record<string, true> {
  if (!(jobId in stoppingJobIds)) return stoppingJobIds;
  const nextStoppingJobIds = { ...stoppingJobIds };
  delete nextStoppingJobIds[jobId];
  return nextStoppingJobIds;
}

function mapRuntimeSummaryToThreadState(
  runtime: ThreadPoolRuntimeSummary,
  existing?: ThreadPoolThreadState,
): ThreadPoolThreadState {
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

function sortThreadPoolThreads(
  threads: ThreadPoolThreadState[],
): ThreadPoolThreadState[] {
  return threads
    .slice()
    .sort((left, right) => {
      const leftTime = left.lastActiveAt ?? "";
      const rightTime = right.lastActiveAt ?? "";
      if (leftTime === rightTime) {
        return right.eventCount - left.eventCount;
      }
      return rightTime.localeCompare(leftTime);
    });
}

function touchThreadPoolThread(
  threads: ThreadPoolThreadState[],
  update: Omit<
    ThreadPoolThreadState,
    | "estimatedMemoryBytes"
    | "lastActiveAt"
    | "recoverable"
    | "lastReason"
    | "eventCount"
  > & {
    estimatedMemoryBytes?: number;
    lastActiveAt?: string;
    recoverable?: boolean;
    reason?: ThreadPoolEventReason | null;
  },
  fallbackSnapshot: ThreadPoolSnapshot | null,
): ThreadPoolThreadState[] {
  const now = update.lastActiveAt ?? new Date().toISOString();
  const existing = threads.find((entry) => entry.threadId === update.threadId);
  const nextEntry: ThreadPoolThreadState = {
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
  return sortThreadPoolThreads([nextEntry, ...withoutCurrent]);
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
  threadPoolSnapshot: null,
  threadPoolSnapshotLoading: false,
  threadPoolError: null,
  threadPoolThreads: [],
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
      void get().refreshThreadPoolSnapshot();
      // NOTE: Do NOT auto-create a session here. Sessions are created only when:
      // 1. User explicitly clicks "New Session" (handleNewSession in session-selector)
      // 2. User sends a message without an active session (sendMessage fallback)
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
        pendingAssistant: null,
        pendingApprovalRequest: null,
        jobStatuses: {},
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
      set({ activeSessionKey: sessionId, errorMessage: null });
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
        pendingAssistant: null,
        pendingApprovalRequest: null,
        jobStatuses: {},
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
    const state = get();
    const agent = state.templates.find((t) => t.id === templateId);
    set({
      selectedTemplateId: templateId,
      selectedProviderPreferenceId: agent?.provider_id ?? null,
      // Apply the agent's configured provider/model as the next-session draft selection.
      selectedModelOverride: agent?.model_id ?? null,
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
          pendingAssistant: {
            content: "",
            reasoning: "",
            toolCalls: [],
            plan: null,
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
            pendingAssistant: null,
            error: errorMessage,
          },
        },
      }));
    }
  },

  async refreshThreadPoolSnapshot() {
    const requestVersion = ++threadPoolSnapshotRequestVersion;
    set((state) => ({
      threadPoolSnapshotLoading: true,
      threadPoolError: null,
      threadPoolSnapshot: state.threadPoolSnapshot,
    }));

    try {
      const poolState = await threadPool.getState();
      set((state) => ({
        ...(requestVersion === threadPoolSnapshotRequestVersion
          ? {
              threadPoolSnapshot: poolState.snapshot,
              threadPoolSnapshotLoading: false,
              threadPoolError: null,
              threadPoolThreads: sortThreadPoolThreads(
                poolState.runtimes.map((runtime) =>
                  mapRuntimeSummaryToThreadState(
                    runtime,
                    state.threadPoolThreads.find(
                      (thread) => thread.threadId === runtime.runtime.thread_id,
                    ),
                  ),
                ),
              ),
            }
          : {}),
      }));
    } catch (error) {
      set(() =>
        requestVersion === threadPoolSnapshotRequestVersion
          ? {
              threadPoolSnapshotLoading: false,
              threadPoolError: toErrorMessage(error),
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
            pendingAssistant: null,
            status: options?.preserveError ? "error" : "idle",
            error: options?.preserveError
              ? state.sessionsByKey[sessionKey].error
              : null,
            tokenCount: snapshot.token_count,
            jobStatuses: state.sessionsByKey[sessionKey].jobStatuses,
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
    const poolHandled = (() => {
      const { payload } = envelope;

      switch (payload.type) {
        case "thread_bound_to_job":
        case "thread_pool_queued":
        case "thread_pool_started":
        case "thread_pool_cooling":
        case "thread_pool_evicted":
        case "thread_pool_metrics_updated":
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
          case "thread_bound_to_job":
            return {
              threadPoolThreads: touchThreadPoolThread(
                state.threadPoolThreads,
                {
                  threadId: envelope.thread_id,
                  kind: "job",
                  sessionId: null,
                  jobId: payload.job_id,
                  status: "queued",
                  reason: null,
                  lastActiveAt: now,
                },
                state.threadPoolSnapshot,
              ),
            };
          case "thread_pool_queued":
            return {
              threadPoolThreads: touchThreadPoolThread(
                state.threadPoolThreads,
                {
                  threadId: envelope.thread_id,
                  kind: payload.runtime.kind,
                  sessionId: payload.runtime.session_id,
                  jobId: payload.runtime.job_id,
                  status: "queued",
                  reason: null,
                  lastActiveAt: now,
                },
                state.threadPoolSnapshot,
              ),
            };
          case "thread_pool_started":
            return {
              threadPoolThreads: touchThreadPoolThread(
                state.threadPoolThreads,
                {
                  threadId: envelope.thread_id,
                  kind: payload.runtime.kind,
                  sessionId: payload.runtime.session_id,
                  jobId: payload.runtime.job_id,
                  status: "running",
                  reason: null,
                  lastActiveAt: now,
                },
                state.threadPoolSnapshot,
              ),
            };
          case "thread_pool_cooling":
            return {
              threadPoolThreads: touchThreadPoolThread(
                state.threadPoolThreads,
                {
                  threadId: envelope.thread_id,
                  kind: payload.runtime.kind,
                  sessionId: payload.runtime.session_id,
                  jobId: payload.runtime.job_id,
                  status: "cooling",
                  reason: null,
                  lastActiveAt: now,
                },
                state.threadPoolSnapshot,
              ),
            };
          case "thread_pool_evicted":
            return {
              threadPoolThreads: touchThreadPoolThread(
                state.threadPoolThreads,
                {
                  threadId: envelope.thread_id,
                  kind: payload.runtime.kind,
                  sessionId: payload.runtime.session_id,
                  jobId: payload.runtime.job_id,
                  status: "evicted",
                  reason: payload.reason,
                  lastActiveAt: now,
                },
                state.threadPoolSnapshot,
              ),
            };
          case "thread_pool_metrics_updated":
            return {
              threadPoolSnapshot: payload.snapshot,
              threadPoolSnapshotLoading: false,
              threadPoolError: null,
              threadPoolThreads: state.threadPoolThreads.map((thread) => ({
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

    const state = get();
    const sessionKey = Object.keys(state.sessionsByKey).find(
      (key) =>
        state.sessionsByKey[key].threadId === envelope.thread_id &&
        state.sessionsByKey[key].sessionId === envelope.session_id,
    );

    if (!sessionKey) return;

    const { payload } = envelope;

    switch (payload.type) {
      case "reasoning_delta":
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session?.pendingAssistant) return {};
          return {
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...session,
                pendingAssistant: {
                  ...session.pendingAssistant,
                  reasoning: session.pendingAssistant.reasoning + payload.delta,
                },
              },
            },
          };
        });
        break;

      case "content_delta":
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session?.pendingAssistant) return {};
          return {
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...session,
                pendingAssistant: {
                  ...session.pendingAssistant,
                  content: session.pendingAssistant.content + payload.delta,
                },
              },
            },
          };
        });
        break;

      case "tool_call_delta": {
        set((state) => {
          const session = state.sessionsByKey[sessionKey];
          if (!session?.pendingAssistant) return {};
          const toolCalls = [...session.pendingAssistant.toolCalls];
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
                ...session,
                pendingAssistant: {
                  ...session.pendingAssistant,
                  toolCalls,
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
          if (!session?.pendingAssistant) return {};
          const existingIndex = session.pendingAssistant.toolCalls.findIndex(
            (tc) => tc.tool_call_id === payload.tool_call_id,
          );
          const toolCalls = [...session.pendingAssistant.toolCalls];
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
              ...session.pendingAssistant,
              toolCalls,
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
                ...session,
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
          if (!session?.pendingAssistant) return {};
          const existingIndex = session.pendingAssistant.toolCalls.findIndex(
            (tc) => tc.tool_call_id === payload.tool_call_id,
          );
          if (existingIndex < 0) return {};
          const toolCalls = [...session.pendingAssistant.toolCalls];
          toolCalls[existingIndex] = {
            ...toolCalls[existingIndex],
            tool_name: payload.tool_name,
            result: payload.result,
            is_error: payload.is_error,
            status: "completed",
          };
          const updates: Partial<ChatSessionState> = {
            pendingAssistant: {
              ...session.pendingAssistant,
              toolCalls,
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
                ...session,
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

      case "turn_completed":
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

      case "notice":
        if (payload.level === "warning" || payload.level === "error") {
          set({ errorMessage: payload.message });
        }
        break;

      case "turn_failed":
        set((store) => ({
          errorMessage: payload.error,
          sessionsByKey: {
            ...store.sessionsByKey,
            [sessionKey]: {
              ...store.sessionsByKey[sessionKey],
              status: "error",
              pendingAssistant: null,
              error: payload.error,
            },
          },
        }));
        void get().refreshSnapshot(sessionKey, { preserveError: true });
        break;

      case "waiting_for_approval":
        set((state) => ({
          sessionsByKey: {
            ...state.sessionsByKey,
            [sessionKey]: {
              ...state.sessionsByKey[sessionKey],
              pendingApprovalRequest: {
                id: payload.request.id,
                tool_name: payload.request.tool_name,
                action: payload.request.action,
                risk_level: payload.request.risk_level,
                requested_at: payload.request.requested_at,
                timeout_secs: payload.request.timeout_secs,
              },
            },
          },
        }));
        break;

      case "approval_resolved":
        set((state) => ({
          sessionsByKey: {
            ...state.sessionsByKey,
            [sessionKey]: {
              ...state.sessionsByKey[sessionKey],
              pendingApprovalRequest: null,
            },
          },
        }));
        break;

      case "idle":
        if (get().sessionsByKey[sessionKey]?.status !== "error") {
          void get().refreshSnapshot(sessionKey);
        }
        break;
    }
  },
}));
