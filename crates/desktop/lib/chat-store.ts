import { create } from "zustand";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { agents, chat, providers } from "@/lib/tauri";
import type {
  ApprovalRequestPayload,
  ThreadEventEnvelope,
  ThreadSnapshotPayload,
} from "@/lib/types/chat";
import type { PlanItem } from "@/lib/types/plan";

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

export interface ChatSessionState {
  sessionId: number;
  templateId: number;
  threadId: string;
  effectiveProviderId: number | null;
  status: "idle" | "running" | "error";
  messages: ThreadSnapshotPayload["messages"];
  pendingAssistant: { content: string; reasoning: string; toolCalls: PendingToolCall[] } | null;
  pendingApprovalRequest: {
    id: string;
    tool_name: string;
    action: string;
    risk_level: ApprovalRequestPayload["risk_level"];
    requested_at: string;
    timeout_secs: number;
  } | null;
  plan: PlanItem[] | null;
  error: string | null;
}

export interface ChatStore {
  selectedTemplateId: number | null;
  selectedProviderPreferenceId: number | null;
  activeSessionId: number | null;
  errorMessage: string | null;
  sessionsByKey: Record<string, ChatSessionState>;
  templates: Awaited<ReturnType<typeof agents.list>>;
  providers: Awaited<ReturnType<typeof providers.list>>;
  _unlisten: UnlistenFn | null;

  initialize: () => Promise<void>;
  activateSession: (sessionId: number, templateId: number, providerId: number | null) => Promise<void>;
  removeSession: (sessionId: number) => void;
  selectTemplateId: (templateId: number | null) => void;
  sendMessage: (content: string) => Promise<void>;
  refreshSnapshot: (sessionId: string) => Promise<void>;
  _handleThreadEvent: (envelope: ThreadEventEnvelope) => void;
}

export const useChatStore = create<ChatStore>((set, get) => ({
  selectedTemplateId: null,
  selectedProviderPreferenceId: null,
  activeSessionId: null,
  errorMessage: null,
  sessionsByKey: {},
  templates: [],
  providers: [],
  _unlisten: null,

  async initialize() {
    if (!get()._unlisten) {
      const unlisten = await listen<ThreadEventEnvelope>("thread:event", (event) => {
        get()._handleThreadEvent(event.payload);
      });
      set({ _unlisten: unlisten });
    }

    try {
      const [templateList, providerList] = await Promise.all([
        agents.list(),
        providers.list(),
      ]);
      set({ templates: templateList, providers: providerList, errorMessage: null });

      if (templateList.length === 0) {
        set({ errorMessage: "当前没有可用的 Agent 模板。" });
        return;
      }

      const firstTemplate = templateList[0];
      // For the initial session, use null sessionId to create a new one
      await get().activateSession(0, firstTemplate.id, null);
    } catch (error) {
      set({ errorMessage: toErrorMessage(error) });
    }
  },

  async activateSession(sessionId: number, templateId: number, providerId: number | null) {
    const sessionKey = sessionId > 0 ? sessionId.toString() : null;
    const state = get();

    // If sessionId is provided, try to reuse existing session
    if (sessionKey && state.sessionsByKey[sessionKey]) {
      set({
        activeSessionId: sessionId,
        selectedTemplateId: templateId,
        selectedProviderPreferenceId: providerId,
        errorMessage: null,
      });
      return;
    }

    try {
      const session = await chat.createChatSession(templateId, providerId);
      const snapshot = await chat.getThreadSnapshot(session.session_id, session.thread_id);

      const newSessionKey = session.session_id.toString();
      const newSessionState: ChatSessionState = {
        sessionId: session.session_id,
        templateId: session.template_id,
        threadId: session.thread_id,
        effectiveProviderId: session.effective_provider_id,
        status: "idle",
        messages: snapshot.messages,
        pendingAssistant: null,
        pendingApprovalRequest: null,
        plan: null,
        error: null,
      };

      set((state) => ({
        activeSessionId: session.session_id,
        selectedTemplateId: templateId,
        selectedProviderPreferenceId: providerId,
        errorMessage: null,
        sessionsByKey: {
          ...state.sessionsByKey,
          [newSessionKey]: newSessionState,
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

  removeSession(sessionId: number) {
    const sessionKey = sessionId.toString();
    set((state) => {
      const entries = Object.entries(state.sessionsByKey).filter(([key]) => key !== sessionKey);
      return {
        sessionsByKey: Object.fromEntries(entries),
        activeSessionId: state.activeSessionId === sessionId ? null : state.activeSessionId,
      };
    });
  },

  selectTemplateId(templateId: number | null) {
    set({ selectedTemplateId: templateId, errorMessage: null });
  },

  async sendMessage(content: string) {
    const trimmedContent = content.trim();
    if (!trimmedContent) return;

    let state = get();
    const activeSessionId = state.activeSessionId;
    const sessionKey = activeSessionId ? activeSessionId.toString() : null;

    if (!sessionKey || !state.sessionsByKey[sessionKey]) {
      const fallbackTemplateId = state.selectedTemplateId ?? state.templates[0]?.id ?? null;
      if (!fallbackTemplateId) {
        set({ errorMessage: "当前没有可用的聊天会话。" });
        return;
      }

      try {
        await get().activateSession(0, fallbackTemplateId, null);
      } catch {
        return;
      }

      state = get();
      const newSessionKey = state.activeSessionId?.toString() ?? null;
      if (!newSessionKey || !state.sessionsByKey[newSessionKey]) {
        set({ errorMessage: "当前会话尚未准备好，请稍后重试。" });
        return;
      }
    }

    const currentSessionKey = state.activeSessionId?.toString() ?? "";
    const session = state.sessionsByKey[currentSessionKey];
    if (!session) {
      set({ errorMessage: "当前会话尚未准备好，请稍后重试。" });
      return;
    }

    set((state) => ({
      errorMessage: null,
      sessionsByKey: {
        ...state.sessionsByKey,
        [currentSessionKey]: {
          ...session,
          status: "running",
          pendingAssistant: { content: "", reasoning: "", toolCalls: [] },
          error: null,
        },
      },
    }));

    try {
      await chat.sendMessage(session.sessionId, session.threadId, trimmedContent);
    } catch (error) {
      const errorMessage = toErrorMessage(error);
      set((store) => ({
        errorMessage,
        sessionsByKey: {
          ...store.sessionsByKey,
          [currentSessionKey]: {
            ...store.sessionsByKey[currentSessionKey],
            status: "error",
            pendingAssistant: null,
            error: errorMessage,
          },
        },
      }));
    }
  },

  async refreshSnapshot(sessionKey: string) {
    const session = get().sessionsByKey[sessionKey];
    if (!session) return;

    try {
      const snapshot = await chat.getThreadSnapshot(session.sessionId, session.threadId);
      set((state) => ({
        errorMessage: null,
        sessionsByKey: {
          ...state.sessionsByKey,
          [sessionKey]: {
            ...state.sessionsByKey[sessionKey],
            messages: snapshot.messages,
            pendingAssistant: null,
            status: "idle",
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

  _handleThreadEvent(envelope: ThreadEventEnvelope) {
    const state = get();
    const sessionKey = Object.keys(state.sessionsByKey).find(
      (key) =>
        state.sessionsByKey[key].threadId === envelope.thread_id &&
        state.sessionsByKey[key].sessionId.toString() === envelope.session_id,
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
          if (payload.arguments_delta !== undefined && payload.arguments_delta !== null) {
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
          if (payload.tool_name === "update_plan" && !payload.is_error && payload.result) {
            const result = payload.result as { plan?: PlanItem[] };
            updates.plan = Array.isArray(result.plan) ? result.plan : null;
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

      case "turn_completed":
        break;

      case "turn_failed":
        set((store) => ({
          errorMessage: payload.error,
          sessionsByKey: {
            ...store.sessionsByKey,
            [sessionKey]: {
              ...store.sessionsByKey[sessionKey],
              status: "error",
              error: payload.error,
            },
          },
        }));
        void get().refreshSnapshot(sessionKey);
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
