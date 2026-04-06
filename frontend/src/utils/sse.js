/**
 * Subscribe to SSE events for a thread.
 * @param {string} threadId
 * @param {string} sessionId
 * @param {function} onEvent - callback(threadEvent) for each SSE message
 * @param {function} onError - callback(error) on connection error
 * @returns {{ close: () => void }}
 */
export function subscribeThreadEvents(threadId, sessionId, onEvent, onError) {
  const url = `/api/threads/${threadId}/events?session_id=${encodeURIComponent(sessionId)}`
  const source = new EventSource(url, { withCredentials: true })

  source.onmessage = (event) => {
    try {
      const threadEvent = JSON.parse(event.data)
      onEvent(threadEvent)
    } catch {
      // ignore malformed events
    }
  }

  source.onerror = (err) => {
    if (onError) onError(err)
  }

  return {
    close() {
      source.close()
    },
  }
}
