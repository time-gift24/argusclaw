import { ref, computed } from "vue";
import {
  getApiClient,
  type ChatSessionSummary,
  type ChatThreadSummary,
  type ChatThreadBinding,
  type ChatSessionPayload,
  type ApiClient,
} from "@/lib/api";

type ChatApiMethods = Required<
  Pick<
    ApiClient,
    | "listChatSessions"
    | "createChatSessionWithThread"
    | "renameChatSession"
    | "deleteChatSession"
    | "listChatThreads"
    | "createChatThread"
    | "renameChatThread"
    | "deleteChatThread"
    | "getChatThreadSnapshot"
    | "updateChatThreadModel"
    | "activateChatThread"
  >
>;

function formatSessionName(session: ChatSessionSummary) {
  const name = session.name.trim();
  return name || `会话 ${session.id.slice(0, 8)}`;
}

function formatThreadTitle(thread: ChatThreadSummary) {
  return thread.title || `线程 ${thread.id.slice(0, 8)}`;
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

async function callChatApi<K extends keyof ChatApiMethods>(
  method: K,
  ...args: Parameters<ChatApiMethods[K]>
): Promise<Awaited<ReturnType<ChatApiMethods[K]>>> {
  const api = getApiClient();
  const fn = api[method] as ChatApiMethods[K] | undefined;
  if (typeof fn !== "function") {
    throw new Error(`当前 API client 未实现 ${String(method)}。`);
  }
  const bound = fn.bind(api) as (...nextArgs: Parameters<ChatApiMethods[K]>) => ReturnType<ChatApiMethods[K]>;
  return (await bound(...args)) as Awaited<ReturnType<ChatApiMethods[K]>>;
}

export function useChatSessions() {
  const sessions = ref<ChatSessionSummary[]>([]);
  const threads = ref<ChatThreadSummary[]>([]);
  const activeSessionId = ref("");
  const activeThreadId = ref("");
  const activeBinding = ref<ChatThreadBinding | null>(null);
  const loading = ref(true);
  const threadLoading = ref(false);
  const creatingThread = ref(false);
  const deleting = ref(false);
  const sessionName = ref("新的 Web 对话");
  const threadTitle = ref("新的对话线程");

  const hasActiveThread = computed(() => Boolean(activeSessionId.value && activeThreadId.value));
  const activeSession = computed(() => sessions.value.find((s) => s.id === activeSessionId.value) ?? null);
  const activeThread = computed(() => threads.value.find((t) => t.id === activeThreadId.value) ?? null);
  const stats = computed(() => ({
    sessions: sessions.value.length,
    threads: threads.value.length,
  }));

  async function loadInitialState() {
    loading.value = true;
    try {
      sessions.value = await callChatApi("listChatSessions");
      if (sessions.value.length > 0) {
        await selectSession(sessions.value[0].id);
      }
    } finally {
      loading.value = false;
    }
  }

  async function refreshSessions() {
    sessions.value = await callChatApi("listChatSessions");
  }

  async function selectSession(sessionId: string) {
    activeSessionId.value = sessionId;
    activeThreadId.value = "";
    activeBinding.value = null;
    const session = sessions.value.find((item) => item.id === sessionId);
    if (session) {
      sessionName.value = formatSessionName(session);
    }
    await refreshThreads();
    if (threads.value.length > 0) {
      selectThread(threads.value[0].id);
    }
  }

  async function refreshThreads() {
    if (!activeSessionId.value) {
      threads.value = [];
      return;
    }
    threads.value = await callChatApi("listChatThreads", activeSessionId.value);
  }

  function selectThread(threadId: string) {
    activeThreadId.value = threadId;
    const thread = threads.value.find((item) => item.id === threadId);
    threadTitle.value = thread?.title ?? "新的对话线程";
  }

  async function renameSession() {
    if (!activeSessionId.value) return;
    const renamed = await callChatApi("renameChatSession", activeSessionId.value, sessionName.value);
    sessions.value = sessions.value.map((s) => (s.id === renamed.id ? renamed : s));
  }

  async function deleteSession() {
    if (!activeSessionId.value || deleting.value) return;
    deleting.value = true;
    try {
      await callChatApi("deleteChatSession", activeSessionId.value);
      await refreshSessions();
      activeSessionId.value = "";
      activeThreadId.value = "";
      threads.value = [];
      if (sessions.value.length > 0) {
        await selectSession(sessions.value[0].id);
      }
    } finally {
      deleting.value = false;
    }
  }

  async function createThread(templateId: number, providerId: number | null, model: string | null) {
    if (!activeSessionId.value) return;
    creatingThread.value = true;
    try {
      const created = await callChatApi("createChatThread", activeSessionId.value, {
        template_id: templateId,
        provider_id: providerId,
        model: model,
      });
      await refreshThreads();
      if (!threads.value.some((t) => t.id === created.id)) {
        threads.value = [created, ...threads.value];
      }
      await selectThread(created.id);
    } finally {
      creatingThread.value = false;
    }
  }

  async function renameThread() {
    if (!activeSessionId.value || !activeThreadId.value) return;
    const renamed = await callChatApi("renameChatThread", activeSessionId.value, activeThreadId.value, threadTitle.value);
    threads.value = threads.value.map((t) => (t.id === renamed.id ? renamed : t));
  }

  async function deleteThread() {
    if (!activeSessionId.value || !activeThreadId.value || deleting.value) return;
    deleting.value = true;
    try {
      await callChatApi("deleteChatThread", activeSessionId.value, activeThreadId.value);
      await refreshThreads();
      activeThreadId.value = "";
      if (threads.value.length > 0) {
        selectThread(threads.value[0].id);
      }
    } finally {
      deleting.value = false;
    }
  }

  async function applyModelBinding(providerId: number, model: string) {
    if (!activeSessionId.value || !activeThreadId.value) return;
    activeBinding.value = await callChatApi("updateChatThreadModel", activeSessionId.value, activeThreadId.value, {
      provider_id: providerId,
      model: model,
    });
  }

  async function activateThread() {
    if (!activeSessionId.value || !activeThreadId.value) return;
    activeBinding.value = await callChatApi("activateChatThread", activeSessionId.value, activeThreadId.value);
  }

  function startNewChatDraft() {
    activeSessionId.value = "";
    activeThreadId.value = "";
    activeBinding.value = null;
    threads.value = [];
    sessionName.value = "新的 Web 对话";
    threadTitle.value = "新的对话线程";
  }

  function applyChatSessionPayload(payload: ChatSessionPayload) {
    activeSessionId.value = payload.session_id;
    activeThreadId.value = payload.thread_id;
    activeBinding.value = {
      session_id: payload.session_id,
      thread_id: payload.thread_id,
      template_id: payload.template_id,
      effective_provider_id: payload.effective_provider_id,
      effective_model: payload.effective_model,
    };
    threadTitle.value = "新的对话线程";
    if (!threads.value.some((t) => t.id === payload.thread_id)) {
      threads.value = [
        {
          id: payload.thread_id,
          title: null,
          turn_count: 0,
          token_count: 0,
          updated_at: new Date().toISOString(),
        },
        ...threads.value,
      ];
    }
  }

  return {
    sessions,
    threads,
    activeSessionId,
    activeThreadId,
    activeBinding,
    loading,
    threadLoading,
    creatingThread,
    deleting,
    sessionName,
    threadTitle,
    hasActiveThread,
    activeSession,
    activeThread,
    stats,
    loadInitialState,
    refreshSessions,
    selectSession,
    refreshThreads,
    selectThread,
    renameSession,
    deleteSession,
    createThread,
    renameThread,
    deleteThread,
    applyModelBinding,
    activateThread,
    startNewChatDraft,
    applyChatSessionPayload,
  };
}

export { formatSessionName, formatThreadTitle, formatDate };
