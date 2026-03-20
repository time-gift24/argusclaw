import { useExternalStoreRuntime } from "@assistant-ui/react";

import { useActiveChatSession } from "@/hooks/use-active-chat-session";
import { PendingToolCall, useChatStore } from "@/lib/chat-store";
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

function pendingToolCallToPart(tc: PendingToolCall): AssistantUiMessagePart {
  return {
    type: "tool-call",
    toolCallId: tc.tool_call_id,
    toolName: tc.tool_name,
    args: undefined,
    argsText: tc.arguments_text,
    result: tc.result,
    isError: tc.is_error,
  };
}

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

    if ((msg.reasoning_content ?? "").trim().length > 0) {
      content.push({
        type: "reasoning",
        text: msg.reasoning_content ?? "",
        parentId: `assistant-reasoning-${index}`,
      });
    }

    if (msg.content.trim().length > 0) {
      content.push({ type: "text", text: msg.content });
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

function buildAssistantUiMessages(session: ReturnType<typeof useActiveChatSession>): AssistantUiMessage[] {
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

    if (session.pendingAssistant.reasoning.trim().length > 0) {
      pendingContent.push({
        type: "reasoning",
        text: session.pendingAssistant.reasoning,
        parentId: `pending-reasoning-${session.threadId}`,
      });
    }

    if (session.pendingAssistant.content) {
      pendingContent.push({ type: "text", text: session.pendingAssistant.content });
    }

    for (const tc of session.pendingAssistant.toolCalls) {
      pendingContent.push(pendingToolCallToPart(tc));
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

  return useExternalStoreRuntime<AssistantUiMessage>({
    isRunning: session?.status === "running",
    messages: buildAssistantUiMessages(session),
    convertMessage: (message) => message,
    onNew: async (message) => {
      await sendMessage(extractUserText(message.content));
    },
  });
}
