import { useExternalStoreRuntime } from "@assistant-ui/react";

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

type AssistantUiMessagePart = {
  readonly type: "text";
  readonly text: string;
};

type TurnToolCall = {
  readonly toolCallId: string;
  readonly toolName: string;
  readonly args?: JsonObject;
  readonly argsText: string;
  readonly result?: unknown;
  readonly isError?: boolean;
};

type TurnArtifacts = {
  readonly reasoning: string;
  readonly toolCalls: readonly TurnToolCall[];
};

type AssistantUiMessage = {
  readonly id: string;
  readonly role: "assistant" | "system" | "user";
  readonly content: string | readonly AssistantUiMessagePart[];
  readonly createdAt: Date;
  readonly status?: { readonly type: "running" };
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

const isFoldedCompactionMessage = (message: ChatMessagePayload) =>
  !!message.metadata?.synthetic &&
  !!message.metadata?.collapsed_by_default &&
  [
    "compaction_prompt",
    "compaction_summary",
    "compaction_replay",
  ].includes(message.metadata.mode ?? "");

type AssistantTurnAccumulator = {
  readonly startedAtIndex: number;
  finalContent: string;
  messageMetadata: ChatMessagePayload["metadata"] | null | undefined;
  reasoningSegments: string[];
  toolCalls: TurnToolCall[];
  toolCallIndexById: Map<string, number>;
};

function buildTextParts(content: string): AssistantUiMessagePart[] {
  if (content.trim().length === 0) return [];
  return [{ type: "text", text: content }];
}

function buildSyntheticMessageDate(seed: number): Date {
  return new Date(seed);
}

function convertNonAssistantSnapshotMessage(
  msg: ChatMessagePayload,
  index: number,
): AssistantUiMessage | null {
  const createdAt = buildSyntheticMessageDate(index);

  if (msg.role === "tool" || msg.role === "assistant") return null;

  if (msg.role === "user") {
    return {
      id: `user-${index}`,
      role: "user",
      content: msg.content,
      createdAt,
      attachments: [],
      metadata: { custom: { messageMetadata: msg.metadata ?? null } },
    };
  }

  return {
    id: `system-${index}`,
    role: "system",
    content: msg.content,
    createdAt,
    metadata: { custom: { messageMetadata: msg.metadata ?? null } },
  };
}

function createAssistantTurnAccumulator(
  msg: ChatMessagePayload,
  index: number,
): AssistantTurnAccumulator {
  const turn: AssistantTurnAccumulator = {
    startedAtIndex: index,
    finalContent: "",
    messageMetadata: msg.metadata ?? null,
    reasoningSegments: [],
    toolCalls: [],
    toolCallIndexById: new Map(),
  };

  appendAssistantMessageToTurn(turn, msg);
  return turn;
}

function appendAssistantMessageToTurn(
  turn: AssistantTurnAccumulator,
  msg: ChatMessagePayload,
) {
  if (msg.content.trim().length > 0) {
    turn.finalContent = msg.content;
  }

  if ((msg.reasoning_content ?? "").trim().length > 0) {
    turn.reasoningSegments.push(msg.reasoning_content ?? "");
  }

  for (const toolCall of msg.tool_calls ?? []) {
    const existingIndex = turn.toolCallIndexById.get(toolCall.id);
    const nextToolCall: TurnToolCall = {
      toolCallId: toolCall.id,
      toolName: toolCall.name,
      args: toReadonlyJsonObject(toolCall.arguments),
      argsText: stringifyValue(toolCall.arguments),
    };

    if (existingIndex === undefined) {
      turn.toolCallIndexById.set(toolCall.id, turn.toolCalls.length);
      turn.toolCalls.push(nextToolCall);
      continue;
    }

    turn.toolCalls[existingIndex] = {
      ...turn.toolCalls[existingIndex]!,
      ...nextToolCall,
    };
  }

  if (msg.metadata) {
    turn.messageMetadata = msg.metadata;
  }
}

function attachToolResultToTurn(
  turn: AssistantTurnAccumulator,
  msg: ChatMessagePayload,
) {
  if (!msg.tool_call_id) return;

  const toolCallIndex = turn.toolCallIndexById.get(msg.tool_call_id);
  if (toolCallIndex === undefined) return;

  turn.toolCalls[toolCallIndex] = {
    ...turn.toolCalls[toolCallIndex]!,
    toolName: msg.name ?? turn.toolCalls[toolCallIndex]!.toolName,
    result: parseMessageContent(msg.content),
  };
}

function buildTurnArtifacts(turn: AssistantTurnAccumulator): TurnArtifacts {
  return {
    reasoning: turn.reasoningSegments.join("\n\n").trim(),
    toolCalls: turn.toolCalls,
  };
}

function flushAssistantTurn(
  messages: AssistantUiMessage[],
  turn: AssistantTurnAccumulator | null,
) {
  if (!turn) return null;

  const turnArtifacts = buildTurnArtifacts(turn);
  const hasText = turn.finalContent.trim().length > 0;
  const hasArtifacts =
    turnArtifacts.reasoning.length > 0 || turnArtifacts.toolCalls.length > 0;

  if (!hasText && !hasArtifacts) return null;

  messages.push({
    id: `assistant-${turn.startedAtIndex}`,
    role: "assistant",
    content: buildTextParts(turn.finalContent),
    createdAt: buildSyntheticMessageDate(turn.startedAtIndex),
    metadata: {
      ...createEmptyAssistantMetadata(),
      custom: {
        messageMetadata: turn.messageMetadata ?? null,
        turnArtifacts,
      },
    },
  });

  return null;
}

function buildPendingUserMessage(
  session: NonNullable<ReturnType<typeof useActiveChatSession>>,
  seed: number,
): AssistantUiMessage | null {
  if (!session.pendingUserMessage) return null;

  return {
    id: `pending-user-${session.threadId}`,
    role: "user",
    content: session.pendingUserMessage,
    createdAt: buildSyntheticMessageDate(seed),
    attachments: [],
    metadata: { custom: { messageMetadata: null } },
  };
}

function buildAggregatedAssistantMessages(
  session: ReturnType<typeof useActiveChatSession>,
): AssistantUiMessage[] {
  if (!session) return [];

  const messages: AssistantUiMessage[] = [];
  let activeAssistantTurn: AssistantTurnAccumulator | null = null;

  for (let index = 0; index < session.messages.length; index += 1) {
    const msg = session.messages[index];

    if (isFoldedCompactionMessage(msg)) {
      let groupEnd = index + 1;
      while (
        groupEnd < session.messages.length &&
        isFoldedCompactionMessage(session.messages[groupEnd]!)
      ) {
        groupEnd += 1;
      }
      const compactionGroup = session.messages.slice(index, groupEnd);
      if (compactionGroup.length > 0) {
        index = groupEnd - 1;
        continue;
      }
    }

    if (msg.role === "assistant") {
      activeAssistantTurn ??= createAssistantTurnAccumulator(msg, index);
      if (activeAssistantTurn.startedAtIndex !== index) {
        appendAssistantMessageToTurn(activeAssistantTurn, msg);
      }
      continue;
    }

    if (msg.role === "tool") {
      if (activeAssistantTurn) {
        attachToolResultToTurn(activeAssistantTurn, msg);
      }
      continue;
    }

    activeAssistantTurn = flushAssistantTurn(messages, activeAssistantTurn);

    const converted = convertNonAssistantSnapshotMessage(msg, index);
    if (!converted) continue;
    messages.push(converted);
  }

  activeAssistantTurn = flushAssistantTurn(messages, activeAssistantTurn);

  const pendingUserMessage = buildPendingUserMessage(session, messages.length + 1);
  if (pendingUserMessage) {
    messages.push(pendingUserMessage);
  }

  if (session.pendingAssistant) {
    messages.push({
      id: `pending-${session.threadId}`,
      role: "assistant",
      content: buildTextParts(session.pendingAssistant.content),
      createdAt: buildSyntheticMessageDate(messages.length + 1),
      status: { type: "running" },
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
    messages: buildAggregatedAssistantMessages(session),
    convertMessage: (message) => message,
    onNew: async (message) => {
      await sendMessage(extractUserText(message.content));
    },
  });
}
