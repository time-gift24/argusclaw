import { useExternalStoreRuntime } from "@assistant-ui/react";

import { useActiveChatSession } from "@/hooks/use-active-chat-session";
import { useChatStore } from "@/lib/chat-store";
import type { ChatMessagePayload } from "@/lib/types/chat";

type AssistantUiMessage =
  | {
      id: string;
      role: "assistant" | "system" | "user";
      content:
        | string
        | Array<
            | { type: "text"; text: string }
            | {
                type: "tool-call";
                toolCallId: string;
                toolName: string;
                args: Record<string, unknown>;
                argsText: string;
              }
          >;
      createdAt: Date;
      status?: { type: "running" } | { type: "requires-action"; reason: "interrupt" };
      attachments?: [];
      metadata?: {
        unstable_state?: null;
        unstable_annotations?: [];
        unstable_data?: [];
        steps?: [];
        custom?: Record<string, unknown>;
      };
    }
  | {
      id: string;
      role: "tool";
      toolCallId: string;
      toolName?: string;
      result: unknown;
      isError?: boolean;
    };

const emptyAssistantMetadata = {
  unstable_state: null,
  unstable_annotations: [],
  unstable_data: [],
  steps: [],
  custom: {},
} as const;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

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

function convertSnapshotMessage(msg: ChatMessagePayload, index: number): AssistantUiMessage[] {
  const createdAt = new Date(index);

  if (msg.role === "tool" && msg.tool_call_id) {
    return [
      {
        id: `tool-${msg.tool_call_id}-${index}`,
        role: "tool",
        toolCallId: msg.tool_call_id,
        toolName: msg.name ?? undefined,
        result: parseMessageContent(msg.content),
      },
    ];
  }

  if (msg.role === "assistant") {
    const content = [];

    if (msg.content.trim().length > 0) {
      content.push({ type: "text" as const, text: msg.content });
    }

    for (const toolCall of msg.tool_calls ?? []) {
      content.push({
        type: "tool-call" as const,
        toolCallId: toolCall.id,
        toolName: toolCall.name,
        args: isRecord(toolCall.arguments) ? toolCall.arguments : { value: toolCall.arguments },
        argsText: stringifyValue(toolCall.arguments),
      });
    }

    return [
      {
        id: `assistant-${index}`,
        role: "assistant",
        content,
        createdAt,
        metadata: emptyAssistantMetadata,
      },
    ];
  }

  if (msg.role === "user") {
    return [
      {
        id: `user-${index}`,
        role: "user",
        content: msg.content,
        createdAt,
        attachments: [],
        metadata: { custom: {} },
      },
    ];
  }

  return [
    {
      id: `system-${index}`,
      role: "system",
      content: msg.content,
      createdAt,
      metadata: { custom: {} },
    },
  ];
}

function buildAssistantUiMessages(session: ReturnType<typeof useActiveChatSession>): AssistantUiMessage[] {
  if (!session) return [];

  const messages = session.messages.flatMap((msg: ChatMessagePayload, index: number) =>
    convertSnapshotMessage(msg, index),
  );

  if (session.pendingAssistant) {
    messages.push({
      id: `pending-${session.threadId}`,
      role: "assistant",
      content: session.pendingAssistant.content
        ? [{ type: "text", text: session.pendingAssistant.content }]
        : [],
      createdAt: new Date(),
      status: session.pendingApprovalRequest
        ? { type: "requires-action", reason: "interrupt" }
        : { type: "running" },
      metadata: emptyAssistantMetadata,
    });
  }

  return messages;
}

function extractUserText(
  content: string | { text?: string } | Array<{ text?: string }>,
): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((part) => part.text ?? "")
      .join("")
      .trim();
  }
  return content.text ?? "";
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
