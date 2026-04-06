import client from './client'

export function listAgents() {
  return client.get('/api/agents')
}

export function listSessions() {
  return client.get('/api/sessions')
}

export function createSession(name) {
  return client.post('/api/sessions', { name })
}

export function listThreads(sessionId) {
  return client.get(`/api/sessions/${sessionId}/threads`)
}

export function sendMessage(threadId, sessionId, content) {
  return client.post(`/api/threads/${threadId}/messages`, {
    session_id: sessionId,
    content,
  })
}
