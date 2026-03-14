"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

// Types matching the Rust backend
export interface ChatMessage {
  role: "user" | "assistant" | "system" | "tool";
  content: string;
  toolCalls?: ToolCall[];
}

export interface ToolCall {
  id: string;
  name: string;
  arguments: unknown;
}

export interface ThreadEvent {
  type: string;
  threadId: string;
  turnNumber?: number;
  event?: LlmStreamEvent;
  toolCallId?: string;
  toolName?: string;
  arguments?: unknown;
  result?: { Ok: unknown } | { Err: string };
  tokenUsage?: TokenUsage;
  error?: string;
  newTokenCount?: number;
}

export interface LlmStreamEvent {
  type: string;
  delta?: string;
  inputTokens?: number;
  outputTokens?: number;
  finishReason?: string;
}

export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
}

export interface UseThreadOptions {
  threadId: string;
  autoSubscribe?: boolean;
}

export interface UseThreadReturn {
  messages: ChatMessage[];
  isRunning: boolean;
  sendMessage: (content: string) => Promise<void>;
  subscribe: () => Promise<void>;
  error: string | null;
}

/**
 * Hook for managing a Thread conversation with the backend.
 *
 * @example
 * ```tsx
 * const { messages, isRunning, sendMessage } = useThread({ threadId: "default" });
 *
 * // Send a message
 * await sendMessage("Hello!");
 *
 * // Messages are automatically updated as events come in
 * ```
 */
export function useThread({
  threadId,
  autoSubscribe = true,
}: UseThreadOptions): UseThreadReturn {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const currentContentRef = useRef<string>("");
  const currentReasoningRef = useRef<string>("");

  // Process incoming thread events
  const handleThreadEvent = useCallback((event: ThreadEvent) => {
    switch (event.type) {
      case "processing":
        if (event.event) {
          const streamEvent = event.event;
          if (streamEvent.type === "contentDelta" && streamEvent.delta) {
            // Accumulate content delta
            currentContentRef.current += streamEvent.delta;

            // Update the last assistant message or create a new one
            setMessages((prev) => {
              const lastMsg = prev[prev.length - 1];
              if (lastMsg?.role === "assistant") {
                return [
                  ...prev.slice(0, -1),
                  { ...lastMsg, content: currentContentRef.current },
                ];
              }
              return [
                ...prev,
                { role: "assistant", content: streamEvent.delta! },
              ];
            });
          } else if (
            streamEvent.type === "reasoningDelta" &&
            streamEvent.delta
          ) {
            // Accumulate reasoning delta (for future display)
            currentReasoningRef.current += streamEvent.delta;
          }
        }
        break;

      case "toolStarted":
        // Tool execution started - could show a loading indicator
        console.log(`Tool started: ${event.toolName}`, event.arguments);
        break;

      case "toolCompleted":
        // Tool execution completed - could show the result
        console.log(`Tool completed: ${event.toolName}`, event.result);
        break;

      case "turnCompleted":
        // Turn completed successfully
        setIsRunning(false);
        currentContentRef.current = "";
        currentReasoningRef.current = "";
        break;

      case "turnFailed":
        // Turn failed
        setIsRunning(false);
        setError(event.error || "Turn failed");
        currentContentRef.current = "";
        currentReasoningRef.current = "";
        break;

      case "idle":
        // Thread is idle
        setIsRunning(false);
        break;

      case "compacted":
        // Context was compacted
        console.log(`Thread compacted, new token count: ${event.newTokenCount}`);
        break;
    }
  }, []);

  // Subscribe to thread events
  const subscribe = useCallback(async () => {
    // Unsubscribe from previous listener if exists
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }

    try {
      // Subscribe to thread events via Tauri IPC
      await invoke("subscribe_thread", { threadId });

      // Listen for thread events
      unlistenRef.current = await listen<string>("thread:event", (event) => {
        try {
          const threadEvent: ThreadEvent = JSON.parse(event.payload);
          handleThreadEvent(threadEvent);
        } catch (e) {
          console.error("Failed to parse thread event:", e);
        }
      });
    } catch (e) {
      setError(`Failed to subscribe: ${e}`);
    }
  }, [threadId, handleThreadEvent]);

  // Auto-subscribe on mount
  useEffect(() => {
    if (autoSubscribe) {
      subscribe();
    }

    return () => {
      if (unlistenRef.current) {
        unlistenRef.current();
      }
    };
  }, [autoSubscribe, subscribe]);

  // Send a message to the thread
  const sendMessage = useCallback(
    async (content: string) => {
      setError(null);
      setIsRunning(true);

      // Add user message to local state
      setMessages((prev) => [
        ...prev,
        { role: "user", content },
      ]);

      try {
        // Send message to backend (non-blocking)
        await invoke("send_message", { threadId, message: content });
      } catch (e) {
        setError(`Failed to send message: ${e}`);
        setIsRunning(false);
      }
    },
    [threadId],
  );

  return {
    messages,
    isRunning,
    sendMessage,
    subscribe,
    error,
  };
}
