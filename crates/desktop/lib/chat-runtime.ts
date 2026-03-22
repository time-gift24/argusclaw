import { useExternalStoreRuntime } from "@assistant-ui/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { useActiveChatSession } from "@/hooks/use-active-chat-session";
import { useChatStore } from "@/lib/chat-store";
import type { ChatMessagePayload } from "@/lib/types/chat";

type JsonValue =
  | null
  | string
  | number
  | boolean
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue };

type JsonObject = { readonly [key: string]: JsonValue };

type AssistantUiMessagePart =
  | {
      readonly type: "text";
      readonly text: string;
    }
  | {
      readonly type: "reasoning";
      readonly text: string;
      readonly parentId?: string;
    }
  | {
      readonly type: "tool-call";
      readonly toolCallId: string;
      readonly toolName: string;
      readonly args?: JsonObject;
      readonly argsText: string;
      readonly result?: unknown;
      readonly isError?: boolean;
      readonly status?: {
        readonly type: "running";
      } | {
        readonly type: "complete";
      } | {
        readonly type: "incomplete";
        readonly reason: "cancelled" | "length" | "error" | "other";
      };
    };

type AssistantUiMessage = {
  readonly id: string;
  readonly role: "assistant" | "system" | "user";
  readonly content: string | readonly AssistantUiMessagePart[];
  readonly createdAt: Date;
  readonly status?:
    | { readonly type: "running" }
    | { readonly type: "requires-action"; readonly reason: "interrupt" };
  readonly attachments?: readonly [];
  readonly metadata?: {
    readonly unstable_state?: JsonValue;
    readonly unstable_annotations?: readonly JsonValue[];
    readonly unstable_data?: readonly JsonValue[];
    readonly steps?: readonly [];
    readonly custom?: Record<string, unknown>;
  };
};

const createEmptyAssistantMetadata = (): NonNullable<AssistantUiMessage["metadata"]> => ({
  unstable_state: null,
  unstable_annotations: [],
  unstable_data: [],
  steps: [],
  custom: {},
});

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const toReadonlyJsonValue = (value: unknown): JsonValue => {
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return value;
  }

  if (Array.isArray(value)) {
    return value.map((item) => toReadonlyJsonValue(item));
  }

  if (isRecord(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, toReadonlyJsonValue(item)]),
    );
  }

  return String(value);
};

const toReadonlyJsonObject = (value: unknown): JsonObject | undefined => {
  if (!isRecord(value)) return undefined;

  return Object.fromEntries(
    Object.entries(value).map(([key, item]) => [key, toReadonlyJsonValue(item)]),
  ) as JsonObject;
};

const stringifyValue = (value: unknown) =>
  typeof value === "string" ? value : JSON.stringify(value ?? {}, null, 2);

const parseMessageContent = (content: string): unknown => {
  const trimmed = content.trim();
  if (!trimmed) return content;

  try {
    return JSON.parse(trimmed);
  } catch {
    return content;
  }
};

function convertSnapshotMessage(msg: ChatMessagePayload, index: number): AssistantUiMessage | null {
  const createdAt = new Date(index);

  if (msg.role === "tool") return null;

  if (msg.role === "assistant") {
    const content: AssistantUiMessagePart[] = [];

    if (msg.content.trim().length > 0) {
      content.push({ type: "text", text: msg.content });
    }

    if ((msg.reasoning_content ?? "").trim().length > 0) {
      content.push({
        type: "reasoning",
        text: msg.reasoning_content ?? "",
        parentId: `assistant-reasoning-${index}`,
      });
    }

    for (const toolCall of msg.tool_calls ?? []) {
      content.push({
        type: "tool-call",
        toolCallId: toolCall.id,
        toolName: toolCall.name,
        args: toReadonlyJsonObject(toolCall.arguments),
        argsText: stringifyValue(toolCall.arguments),
      });
    }

    return {
      id: `assistant-${index}`,
      role: "assistant",
      content,
      createdAt,
      metadata: createEmptyAssistantMetadata(),
    };
  }

  if (msg.role === "user") {
    return {
      id: `user-${index}`,
      role: "user",
      content: msg.content,
      createdAt,
      attachments: [],
      metadata: { custom: {} },
    };
  }

  return {
    id: `system-${index}`,
    role: "system",
    content: msg.content,
    createdAt,
    metadata: { custom: {} },
  };
}

function buildAssistantUiMessages(
  session: ReturnType<typeof useActiveChatSession>,
  localPendingMessages: readonly AssistantUiMessage[] = [],
): AssistantUiMessage[] {
  if (!session) return [];

  const messages: AssistantUiMessage[] = [];
  const toolCallLocations = new Map<
    string,
    {
      messageIndex: number;
      partIndex: number;
    }
  >();

  session.messages.forEach((msg: ChatMessagePayload, index: number) => {
    if (msg.role === "tool" && msg.tool_call_id) {
      const location = toolCallLocations.get(msg.tool_call_id);
      if (!location) return;

      const targetMessage = messages[location.messageIndex];
      if (!targetMessage || !Array.isArray(targetMessage.content)) return;

      const nextContent = targetMessage.content.map((part, partIndex) => {
        if (partIndex !== location.partIndex || part.type !== "tool-call") return part;

        return {
          ...part,
          toolName: msg.name ?? part.toolName,
          result: parseMessageContent(msg.content),
        };
      });

      messages[location.messageIndex] = {
        ...targetMessage,
        content: nextContent,
      };
      return;
    }

    const converted = convertSnapshotMessage(msg, index);
    if (!converted) return;

    const messageIndex = messages.push(converted) - 1;
    if (converted.role !== "assistant" || !Array.isArray(converted.content)) return;

    converted.content.forEach((part, partIndex) => {
      if (part.type === "tool-call") {
        toolCallLocations.set(part.toolCallId, { messageIndex, partIndex });
      }
    });
  });

  if (session.pendingAssistant) {
    const pendingContent: AssistantUiMessagePart[] = [];

    if (session.pendingAssistant.content) {
      pendingContent.push({ type: "text", text: session.pendingAssistant.content });
    }

    if (session.pendingAssistant.reasoning.trim().length > 0) {
      pendingContent.push({
        type: "reasoning",
        text: session.pendingAssistant.reasoning,
        parentId: `pending-reasoning-${session.threadId}`,
      });
    }

    // Add tool calls from pending assistant
    for (const toolCall of session.pendingAssistant.toolCalls ?? []) {
      const toolStatus =
        toolCall.status === "running"
          ? { type: "running" as const }
          : toolCall.status === "completed"
            ? { type: "complete" as const }
            : { type: "incomplete" as const, reason: "error" as const };

      pendingContent.push({
        type: "tool-call" as const,
        toolCallId: toolCall.tool_call_id,
        toolName: toolCall.tool_name,
        args: undefined,
        argsText: toolCall.arguments_text,
        result: toolCall.result,
        isError: toolCall.is_error,
        status: toolStatus,
      });
    }

    messages.push({
      id: `pending-${session.threadId}`,
      role: "assistant",
      content: pendingContent,
      createdAt: new Date(),
      status: session.pendingApprovalRequest
        ? { type: "requires-action", reason: "interrupt" }
        : { type: "running" },
      metadata: createEmptyAssistantMetadata(),
    });

    // Insert local pending messages BEFORE the pending assistant (user message comes first chronologically)
    if (localPendingMessages.length > 0) {
      // Insert right before the pending assistant message
      const pendingAssistantIndex = messages.findIndex(
        (msg) => msg.id === `pending-${session.threadId}`,
      );
      if (pendingAssistantIndex > 0) {
        messages.splice(pendingAssistantIndex, 0, ...localPendingMessages);
      } else {
        messages.push(...localPendingMessages);
      }
    }
  } else if (localPendingMessages.length > 0) {
    // No pending assistant, just append
    messages.push(...localPendingMessages);
  }

  return messages;
}

function extractUserText(
  content:
    | string
    | { text?: string }
    | ReadonlyArray<unknown>,
): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((part) =>
        isRecord(part) && typeof part.text === "string" ? part.text : "",
      )
      .join("")
      .trim();
  }
  if (isRecord(content) && typeof content.text === "string") {
    return content.text ?? "";
  }

  return "";
}

export function useChatRuntime(): ReturnType<typeof useExternalStoreRuntime<AssistantUiMessage>> {
  const sendMessage = useChatStore((state) => state.sendMessage);
  const session = useActiveChatSession();
  const [localPendingMessages, setLocalPendingMessages] = useState<readonly AssistantUiMessage[]>([]);
  const pendingIdsRef = useRef<Set<string>>(new Set());
  const prevPendingAssistantRef = useRef(session?.pendingAssistant);

  const messages = useMemo(
    () => buildAssistantUiMessages(session, localPendingMessages),
    [session?.messages, session?.pendingAssistant, session?.pendingApprovalRequest, localPendingMessages],
  );

  // Clear pending messages when a new turn STARTS (pendingAssistant goes from null to having content)
  // This prevents scroll reset when turn completes
  useEffect(() => {
    const prev = prevPendingAssistantRef.current;
    const current = session?.pendingAssistant;

    // New turn started: prev was null/falsy and current is truthy
    if (!prev && current) {
      setLocalPendingMessages([]);
      pendingIdsRef.current.clear();
    }

    prevPendingAssistantRef.current = current ?? undefined;
  }, [session?.pendingAssistant]);

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const handleSend = useCallback(async (message: any) => {
      const text = extractUserText(message.content);
      const tempId = `pending-user-${Date.now()}`;

      // Immediately add the message optimistically
      const pendingMsg: AssistantUiMessage = {
        id: tempId,
        role: "user",
        content: text,
        createdAt: new Date(),
        attachments: [],
        metadata: { custom: { pending: true } },
      };

      pendingIdsRef.current.add(tempId);
      setLocalPendingMessages((prev) => [...prev, pendingMsg]);

      // Send to backend (don't await for instant display)
      sendMessage(text).catch((err) => {
        // On error, remove the pending message
        console.error("Failed to send message:", err);
        setLocalPendingMessages((prev) => prev.filter((msg) => msg.id !== tempId));
        pendingIdsRef.current.delete(tempId);
      });
    },
    [sendMessage],
  );

  return useExternalStoreRuntime<AssistantUiMessage>({
    isRunning: session?.status === "running",
    messages,
    convertMessage: (message) => message,
    onNew: handleSend,
  });
}
