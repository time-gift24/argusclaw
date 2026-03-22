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

const toSessionKey = (templateId: number, providerPreferenceId: number | null) =>
  `${templateId}::${providerPreferenceId ?? "__default__"}`;

const toErrorMessage = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

export interface ChatSessionState {
  sessionKey: string;
  sessionId: number;
  templateId: number;
  threadId: string;
  effectiveProviderId: number | null;
  status: "idle" | "running" | "error";
  messages: ThreadSnapshotPayload["messages"];
  pendingAssistant: { content: string; reasoning: string; toolCalls: PendingToolCall[]; plan: PlanItem[] | null } | null;
  pendingApprovalRequest: {
    id: string;
    tool_name: string;
    action: string;
    risk_level: ApprovalRequestPayload["risk_level"];
    requested_at: string;
    timeout_secs: number;
  } | null;
  error: string | null;
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
  _unlisten: UnlistenFn | null;

  initialize: () => Promise<void>;
  activateSession: (templateId: number) => Promise<void>;
  selectProviderPreference: (providerId: number | null) => Promise<void>;
  selectModelOverride: (model: string | null) => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  refreshSnapshot: (sessionKey: string) => Promise<void>;
  cleanup: () => void;
  _handleThreadEvent: (envelope: ThreadEventEnvelope) => void;
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
      await get().activateSession(firstTemplate.id);
    } catch (error) {
      set({ errorMessage: toErrorMessage(error) });
    }
  },

  async activateSession(templateId: number) {
    const state = get();
    const sessionKey = toSessionKey(templateId, state.selectedProviderPreferenceId);

    // Reuse existing session if available
    if (state.sessionsByKey[sessionKey]) {
      set({
        activeSessionKey: sessionKey,
        selectedTemplateId: templateId,
        errorMessage: null,
      });
      return;
    }

    try {
      const session = await chat.createChatSession(
        templateId,
        state.selectedProviderPreferenceId,
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
        status: "idle",
        messages: snapshot.messages,
        pendingAssistant: null,
        pendingApprovalRequest: null,
        error: null,
      };

      set((state) => ({
        activeSessionKey: sessionKey,
        selectedTemplateId: templateId,
        errorMessage: null,
        sessionsByKey: {
          ...state.sessionsByKey,
          [sessionKey]: newSessionState,
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

  async selectProviderPreference(providerId: number | null) {
    set({ selectedProviderPreferenceId: providerId, errorMessage: null });

    const state = get();
    if (state.selectedTemplateId) {
      try {
        await get().activateSession(state.selectedTemplateId);
      } catch {
        // activateSession already populated the visible error state
      }
    }
  },

  async selectModelOverride(model: string | null) {
    set({ selectedModelOverride: model, errorMessage: null });

    const state = get();
    if (state.selectedTemplateId) {
      try {
        await get().activateSession(state.selectedTemplateId);
      } catch {
        // activateSession already populated the visible error state
      }
    }
  },

  async sendMessage(content: string) {
    const trimmedContent = content.trim();
    if (!trimmedContent) return;

    let state = get();
    if (!state.activeSessionKey) {
      const fallbackTemplateId = state.selectedTemplateId ?? state.templates[0]?.id ?? null;
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
          pendingAssistant: { content: "", reasoning: "", toolCalls: [], plan: null },
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

  cleanup() {
    const unlisten = get()._unlisten;
    if (unlisten) {
      unlisten();
      set({ _unlisten: null });
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
              plan: payload.is_error ? null : Array.isArray(result?.plan) ? result.plan : null,
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
