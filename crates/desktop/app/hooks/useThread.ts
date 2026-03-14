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
const isTauriEnv = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

console.log("[useThread] Module loaded, isTauriEnv:", isTauriEnv);

/**
 * Hook for managing a Thread conversation with the backend.
 */
export function useThread({
  threadId,
  autoSubscribe = true,
}: UseThreadOptions): UseThreadReturn {
  console.log("[useThread] Hook called with threadId:", threadId, "autoSubscribe:", autoSubscribe);

  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const currentContentRef = useRef<string>("");
  const currentReasoningRef = useRef<string>("");

  // Process incoming thread events
  const handleThreadEvent = useCallback((event: ThreadEvent) => {
    console.log("[useThread] Received event:", event.type, event);
    switch (event.type) {
      case "processing":
        if (event.event) {
          const streamEvent = event.event;
          console.log("[useThread] Stream event:", streamEvent.type, streamEvent.delta);
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
        console.log(`[useThread] Tool started: ${event.toolName}`, event.arguments);
        break;

      case "toolCompleted":
        console.log(`[useThread] Tool completed: ${event.toolName}`, event.result);
        break;

      case "turnCompleted":
        console.log("[useThread] Turn completed");
        setIsRunning(false);
        currentContentRef.current = "";
        currentReasoningRef.current = "";
        break;

      case "turnFailed":
        console.log("[useThread] Turn failed:", event.error);
        setIsRunning(false);
        setError(event.error || "Turn failed");
        currentContentRef.current = "";
        currentReasoningRef.current = "";
        break;

      case "idle":
        console.log("[useThread] Thread idle");
        setIsRunning(false);
        break;

      case "compacted":
        console.log(`[useThread] Thread compacted, new token count: ${event.newTokenCount}`);
        break;
    }
  }, []);

  // Subscribe to thread events
  const subscribe = useCallback(async () => {
    console.log("[useThread] subscribe() called, threadId:", threadId);
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }

    if (!isTauriEnv) {
      console.log("[useThread] Not in Tauri environment, skipping subscription");
      return;
    }

    try {
      console.log("[useThread] Calling invoke subscribe_thread with threadId:", threadId);
      await invoke("subscribe_thread", { threadId });
      console.log("[useThread] subscribe_thread succeeded, setting up listener");
      unlistenRef.current = await listen<string>("thread:event", (event) => {
        try {
          console.log("[useThread] Raw event payload:", event.payload);
          const threadEvent: ThreadEvent = JSON.parse(event.payload);
          handleThreadEvent(threadEvent);
        } catch (e) {
          console.error("[useThread] Failed to parse thread event:", e);
        }
      });
      console.log("[useThread] Listener setup complete");
    } catch (e) {
        console.error("[useThread] Failed to subscribe:", e);
    }
  }, [threadId, handleThreadEvent]);

  // Auto-subscribe on mount
  useEffect(() => {
    console.log("[useThread] useEffect - autoSubscribe:", autoSubscribe);
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
      console.log("[useThread] sendMessage called with:", content);
      setError(null);
      setIsRunning(true);
      setMessages((prev) => [...prev, { role: "user", content }]);

      if (!isTauriEnv) {
        // Mock response for development
        console.log("[useThread] Not in Tauri environment, simulating response...");
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
        console.log("[useThread] Calling invoke send_message with threadId:", threadId, "message:", content);
        await invoke("send_message", { threadId, message: content });
        console.log("[useThread] send_message succeeded");
      } catch (e) {
        console.error("[useThread] Failed to send message:", e);
        setError(`Failed to send message: ${e}`);
        setIsRunning(false);
      }
    },
    [threadId],
  );

  return { messages, isRunning, sendMessage, subscribe, error };
}
