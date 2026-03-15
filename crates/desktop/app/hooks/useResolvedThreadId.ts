"use client";

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export const FALLBACK_THREAD_ID = "00000000-0000-0000-0000-000000000001";

const isTauriEnv =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

interface DefaultThreadInfo {
  thread_id: string;
  agent_runtime_id: string;
}

interface UseResolvedThreadIdResult {
  error: string | null;
  isReady: boolean;
  threadId: string | null;
}

export function useResolvedThreadId(
  initialThreadId?: string,
): UseResolvedThreadIdResult {
  const [resolvedThreadId, setResolvedThreadId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (initialThreadId || !isTauriEnv) {
      return;
    }

    let cancelled = false;

    invoke<DefaultThreadInfo>("get_default_thread_id")
      .then((info) => {
        if (cancelled) return;
        setResolvedThreadId(info.thread_id);
        setError(null);
      })
      .catch((invokeError) => {
        if (cancelled) return;
        setResolvedThreadId(null);
        setError(`Failed to resolve default thread: ${invokeError}`);
      });

    return () => {
      cancelled = true;
    };
  }, [initialThreadId]);

  if (initialThreadId) {
    return { error: null, isReady: true, threadId: initialThreadId };
  }

  if (!isTauriEnv) {
    return { error: null, isReady: true, threadId: FALLBACK_THREAD_ID };
  }

  return {
    error,
    isReady: resolvedThreadId !== null,
    threadId: resolvedThreadId,
  };
}
