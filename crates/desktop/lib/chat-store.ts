import { create } from "zustand";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { agents, chat, models, providers } from "@/lib/tauri";
import type {
  ApprovalRequestPayload,
  ThreadEventEnvelope,
  ThreadSnapshotPayload,
} from "@/lib/types/chat";

const toSessionKey = (templateId: string, providerPreferenceId: string | null) =>
  `${templateId}::${providerPreferenceId ?? "__default__"}`;

const toErrorMessage = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

export interface ChatSessionState {
  sessionKey: string;
  templateId: string;
  runtimeAgentId: string;
  threadId: string;
  effectiveProviderId: string;
  status: "idle" | "running" | "error";
  messages: ThreadSnapshotPayload["messages"];
  pendingAssistant: { content: string; reasoning: string } | null;
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

export interface ProviderModel {
  providerId: string;
  providerName: string;
  modelId: string;
  modelName: string;
}

export interface ChatStore {
  selectedTemplateId: string | null;
  selectedProviderPreferenceId: string | null;
  activeSessionKey: string | null;
  errorMessage: string | null;
  sessionsByKey: Record<string, ChatSessionState>;
  templates: Awaited<ReturnType<typeof agents.list>>;
  providers: Awaited<ReturnType<typeof providers.list>>;
  providerModels: ProviderModel[];
  _unlisten: UnlistenFn | null;

  initialize: () => Promise<void>;
  activateSession: (templateId: string) => Promise<void>;
  selectProviderPreference: (providerModelId: string | null) => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  refreshSnapshot: (sessionKey: string) => Promise<void>;
  cleanup: () => void;
  _handleThreadEvent: (envelope: ThreadEventEnvelope) => void;
}

export const useChatStore = create<ChatStore>((set, get) => ({
  selectedTemplateId: null,
  selectedProviderPreferenceId: null,
  activeSessionKey: null,
  errorMessage: null,
  sessionsByKey: {},
  templates: [],
  providers: [],
  providerModels: [],
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

      // Load all models from all providers
      const allModels: ProviderModel[] = [];
      for (const provider of providerList) {
        try {
          const providerModels = await models.listByProvider(provider.id);
          for (const model of providerModels) {
            allModels.push({
              providerId: provider.id,
              providerName: provider.display_name,
              modelId: model.id,
              modelName: model.name,
            });
          }
        } catch (error) {
          console.error(`Failed to load models for provider ${provider.id}:`, error);
        }
      }

      set({
        templates: templateList,
        providers: providerList,
        providerModels: allModels,
        errorMessage: null,
      });

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

  async activateSession(templateId: string) {
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
      const session = await chat.createChatSession(templateId, state.selectedProviderPreferenceId);
      const snapshot = await chat.getThreadSnapshot(session.runtime_agent_id, session.thread_id);

      const newSessionState: ChatSessionState = {
        sessionKey: session.session_key,
        templateId: session.template_id,
        runtimeAgentId: session.runtime_agent_id,
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

  async selectProviderPreference(providerModelId: string | null) {
    // Extract providerId from providerModelId (format: "providerId:modelId" or just "providerId" for default)
    const providerId = providerModelId?.includes(":")
      ? providerModelId.split(":")[0]
      : providerModelId;

    set({ selectedProviderPreferenceId: providerModelId, errorMessage: null });

    const state = get();
    if (state.selectedTemplateId) {
      try {
        // Pass providerId (not providerModelId) to activateSession for session key
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
          pendingAssistant: { content: "", reasoning: "" },
          error: null,
        },
      },
    }));

    try {
      await chat.sendMessage(session.runtimeAgentId, session.threadId, trimmedContent);
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
      const snapshot = await chat.getThreadSnapshot(session.runtimeAgentId, session.threadId);
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
        state.sessionsByKey[key].runtimeAgentId === envelope.runtime_agent_id,
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
