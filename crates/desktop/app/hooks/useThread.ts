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

// Check if running in Tauri environment
const isTauriEnv = typeof window !== "undefined" && "__TAURI__" in window;

/**
 * Hook for managing a Thread conversation with the backend.
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
            currentContentRef.current += streamEvent.delta;
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
          } else if (streamEvent.type === "reasoningDelta" && streamEvent.delta) {
            currentReasoningRef.current += streamEvent.delta;
          }
        }
        break;

      case "toolStarted":
        console.log(`Tool started: ${event.toolName}`, event.arguments);
        break;

      case "toolCompleted":
        console.log(`Tool completed: ${event.toolName}`, event.result);
        break;

      case "turnCompleted":
        setIsRunning(false);
        currentContentRef.current = "";
        currentReasoningRef.current = "";
        break;

      case "turnFailed":
        setIsRunning(false);
        setError(event.error || "Turn failed");
        currentContentRef.current = "";
        currentReasoningRef.current = "";
        break;

      case "idle":
        setIsRunning(false);
        break;

      case "compacted":
        console.log(`Thread compacted, new token count: ${event.newTokenCount}`);
        break;
    }
  }, []);

  // Subscribe to thread events
  const subscribe = useCallback(async () => {
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }

    if (!isTauriEnv) {
      console.log("Not in Tauri environment, skipping subscription");
      return;
    }

    try {
      await invoke("subscribe_thread", { threadId });
      unlistenRef.current = await listen<string>("thread:event", (event) => {
        try {
          const threadEvent: ThreadEvent = JSON.parse(event.payload);
          handleThreadEvent(threadEvent);
        } catch (e) {
          console.error("Failed to parse thread event:", e);
        }
      });
    } catch (e) {
        console.error("Failed to subscribe:", e);
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
      setMessages((prev) => [...prev, { role: "user", content }]);

      if (!isTauriEnv) {
        // Mock response for development
        console.log("Not in Tauri environment, simulating response...");
        setTimeout(() => {
          setMessages((prev) => [
            ...prev,
            {
              role: "assistant",
              content: "This is a mock response. Please run in Tauri to connect to the backend.",
            },
          ]);
          setIsRunning(false);
        }, 1000);
        return;
      }

      try {
        await invoke("send_message", { threadId, message: content });
      } catch (e) {
        setError(`Failed to send message: ${e}`);
        setIsRunning(false);
      }
    },
    [threadId],
  );

  return { messages, isRunning, sendMessage, subscribe, error };
}
