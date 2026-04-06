import { ref, computed } from 'vue'
import { defineStore } from 'pinia'
import { listSessions, createSession, listThreads, listAgents, sendMessage } from '../api/chat'
import { subscribeThreadEvents } from '../utils/sse'

export const useChatStore = defineStore('chat', () => {
  const sessions = ref([])
  const currentSessionId = ref(null)
  const threads = ref([])
  const currentThreadId = ref(null)
  const agents = ref([])
  const messages = ref([])
  const sending = ref(false)
  const sseController = ref(null)

  const currentSession = computed(() =>
    sessions.value.find((s) => s.id === currentSessionId.value)
  )

  async function fetchSessions() {
    const { data } = await listSessions()
    sessions.value = data
  }

  async function newSession(name) {
    const { data } = await createSession(name)
    await fetchSessions()
    currentSessionId.value = data.id
    return data.id
  }

  function selectSession(sessionId) {
    currentSessionId.value = sessionId
    threads.value = []
    currentThreadId.value = null
    messages.value = []
  }

  async function fetchThreads() {
    if (!currentSessionId.value) return
    const { data } = await listThreads(currentSessionId.value)
    threads.value = data
  }

  function selectThread(threadId) {
    currentThreadId.value = threadId
    messages.value = []
    connectSSE(threadId, currentSessionId.value)
  }

  async function fetchAgents() {
    const { data } = await listAgents()
    agents.value = data
  }

  async function send(content) {
    if (!currentThreadId.value || !currentSessionId.value) return
    sending.value = true

    messages.value.push({
      id: `user-${Date.now()}`,
      role: 'user',
      content,
    })

    const aiMsgId = `ai-${Date.now()}`
    messages.value.push({
      id: aiMsgId,
      role: 'ai',
      content: '',
      loading: true,
    })

    try {
      await sendMessage(currentThreadId.value, currentSessionId.value, content)
    } catch {
      const idx = messages.value.findIndex((m) => m.id === aiMsgId)
      if (idx >= 0) {
        messages.value[idx] = { ...messages.value[idx], loading: false, error: true }
      }
    } finally {
      sending.value = false
    }
  }

  function connectSSE(threadId, sessionId) {
    disconnectSSE()
    sseController.value = subscribeThreadEvents(threadId, sessionId, handleSSEEvent)
  }

  function disconnectSSE() {
    if (sseController.value) {
      sseController.value.close()
      sseController.value = null
    }
  }

  function handleSSEEvent(threadEvent) {
    const lastAiMsg = [...messages.value].reverse().find((m) => m.role === 'ai')

    // ThreadEvent from argus-protocol is tagged enum: { "Processing": { ... } } or { "TurnCompleted": { ... } }
    // The SSE handler in argus-server serializes ThreadEvent with serde_json
    // We detect the variant by checking which key exists
    if (threadEvent.Processing) {
      const evt = threadEvent.Processing
      if (lastAiMsg) {
        // LlmStreamEvent has a delta/content field for text chunks
        const delta = evt.event?.delta || evt.event?.content || ''
        lastAiMsg.content += delta
        lastAiMsg.loading = false
      }
    } else if (threadEvent.ToolStarted) {
      const evt = threadEvent.ToolStarted
      messages.value.push({
        id: `tool-${Date.now()}`,
        role: 'tool',
        toolName: evt.tool_name,
        arguments: evt.arguments,
        loading: true,
      })
    } else if (threadEvent.ToolCompleted) {
      const evt = threadEvent.ToolCompleted
      const toolMsg = [...messages.value].reverse().find((m) => m.role === 'tool' && m.loading)
      if (toolMsg) {
        toolMsg.result = evt.result
        toolMsg.loading = false
      }
    } else if (threadEvent.TurnCompleted) {
      if (lastAiMsg) lastAiMsg.loading = false
    } else if (threadEvent.TurnFailed) {
      const evt = threadEvent.TurnFailed
      if (lastAiMsg) {
        lastAiMsg.loading = false
        lastAiMsg.error = true
        lastAiMsg.content = evt.error || '处理失败'
      }
    } else if (threadEvent.Idle) {
      if (lastAiMsg) lastAiMsg.loading = false
    }
  }

  return {
    sessions, currentSessionId, threads, currentThreadId,
    agents, messages, sending, currentSession,
    fetchSessions, newSession, selectSession,
    fetchThreads, selectThread, fetchAgents,
    send, disconnectSSE,
  }
})
