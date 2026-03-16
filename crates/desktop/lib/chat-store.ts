import { create } from "zustand";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { agents, chat, providers } from "@/lib/tauri";
import type {
  ApprovalRequestPayload,
  ThreadEventEnvelope,
  ThreadSnapshotPayload,
} from "@/lib/types/chat";

const toSessionKey = (templateId: string, providerPreferenceId: string | null) =>
  `${templateId}::${providerPreferenceId ?? "__default__"}`;

export interface ChatSessionState {
  sessionKey: string;
  templateId: string;
  runtimeAgentId: string;
  threadId: string;
  effectiveProviderId: string;
  status: "idle" | "running" | "error";
  messages: ThreadSnapshotPayload["messages"];
  pendingAssistant: { content: string } | null;
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
  selectedTemplateId: string | null;
  selectedProviderPreferenceId: string | null;
  activeSessionKey: string | null;
  sessionsByKey: Record<string, ChatSessionState>;
  templates: Awaited<ReturnType<typeof agents.list>>;
  providers: Awaited<ReturnType<typeof providers.list>>;
  _unlisten: UnlistenFn | null;

  initialize: () => Promise<void>;
  activateSession: (templateId: string) => Promise<void>;
  selectProviderPreference: (providerId: string | null) => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  refreshSnapshot: (sessionKey: string) => Promise<void>;
  cleanup: () => void;
  _handleThreadEvent: (envelope: ThreadEventEnvelope) => void;
}

export const useChatStore = create<ChatStore>((set, get) => ({
  selectedTemplateId: null,
  selectedProviderPreferenceId: null,
  activeSessionKey: null,
  sessionsByKey: {},
  templates: [],
  providers: [],
  _unlisten: null,

  async initialize() {
    const [templateList, providerList] = await Promise.all([
      agents.list(),
      providers.list(),
    ]);
    set({ templates: templateList, providers: providerList });

    // Auto-activate first template if available
    if (templateList.length > 0) {
      const firstTemplate = templateList[0];
      await get().activateSession(firstTemplate.id);
    }

    if (!get()._unlisten) {
      const unlisten = await listen<ThreadEventEnvelope>("thread:event", (event) => {
        get()._handleThreadEvent(event.payload);
      });
      set({ _unlisten: unlisten });
    }
  },

  async activateSession(templateId: string) {
    const state = get();
    const sessionKey = toSessionKey(templateId, state.selectedProviderPreferenceId);

    // Reuse existing session if available
    if (state.sessionsByKey[sessionKey]) {
      set({ activeSessionKey: sessionKey, selectedTemplateId: templateId });
      return;
    }

    // Create new session
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
      sessionsByKey: {
        ...state.sessionsByKey,
        [sessionKey]: newSessionState,
      },
    }));
  },

  async selectProviderPreference(providerId: string | null) {
    set({ selectedProviderPreferenceId: providerId });

    const state = get();
    if (state.selectedTemplateId) {
      // Activate or create session with new provider preference
      await get().activateSession(state.selectedTemplateId);
    }
  },

  async sendMessage(content: string) {
    const state = get();
    if (!state.activeSessionKey) return;

    const session = state.sessionsByKey[state.activeSessionKey];
    if (!session) return;

    set((state) => ({
      sessionsByKey: {
        ...state.sessionsByKey,
        [state.activeSessionKey!]: {
          ...session,
          status: "running",
          pendingAssistant: { content: "" },
        },
      },
    }));

    await chat.sendMessage(session.runtimeAgentId, session.threadId, content);
  },

  async refreshSnapshot(sessionKey: string) {
    const session = get().sessionsByKey[sessionKey];
    if (!session) return;

    const snapshot = await chat.getThreadSnapshot(session.runtimeAgentId, session.threadId);
    set((state) => ({
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
                  content: session.pendingAssistant.content + payload.delta,
                },
              },
            },
          };
        });
        break;

      case "turn_completed":
        void get().refreshSnapshot(sessionKey);
        break;

      case "turn_failed":
        set((store) => ({
          sessionsByKey: {
            ...store.sessionsByKey,
            [sessionKey]: {
              ...store.sessionsByKey[sessionKey],
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
        set((state) => ({
          sessionsByKey: {
            ...state.sessionsByKey,
            [sessionKey]: {
              ...state.sessionsByKey[sessionKey],
              status: "idle",
            },
          },
        }));
        break;
    }
  },
}));
