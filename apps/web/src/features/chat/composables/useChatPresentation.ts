import { h } from "vue";
import type {
  BubbleRoleConfig,
  ChatMessageContent,
  ChatMessageContentItem,
  PromptProps,
} from "@opentiny/tiny-robot";

import type { ChatMessageRecord } from "@/lib/api";
import type {
  ToolActivity,
  ToolActivityStatus,
  TurnTimelineItem,
} from "./useChatThreadStream";

export const TOOL_SUMMARY_CONTENT_TYPE = "argus-tool-summary";
export const TURN_TIMELINE_CONTENT_TYPE = "argus-turn-timeline";

export interface ToolCallDetail {
  id: string;
  kind: "shell" | "mcp" | "search" | "http" | "file" | "job" | "tool";
  name: string;
  status: ToolActivityStatus;
  inputPreview: string;
  outputPreview: string;
}

export interface ToolSummaryContentItem extends ChatMessageContentItem {
  type: typeof TOOL_SUMMARY_CONTENT_TYPE;
  toolDetails: ToolCallDetail[];
}

export interface TurnTimelineContentItem extends ChatMessageContentItem {
  type: typeof TURN_TIMELINE_CONTENT_TYPE;
  items: TurnTimelineItem[];
}

export interface TextContentItem extends ChatMessageContentItem {
  type: "text";
  text: string;
}

export type ChatRobotContentItem =
  | ToolSummaryContentItem
  | TurnTimelineContentItem
  | TextContentItem;

export interface ChatRobotMessageState extends Record<string, unknown> {
  thinking?: boolean;
  open?: boolean;
  toolDetails?: ToolCallDetail[];
}

export interface ChatRobotMessage {
  id?: string;
  role: string;
  content: ChatMessageContent;
  reasoning_content?: string;
  loading?: boolean;
  state?: ChatRobotMessageState;
}

export interface RobotMessageOptions {
  messages: ChatMessageRecord[];
  streaming: boolean;
  hasActiveThread: boolean;
  pendingAssistantContent: string;
  pendingAssistantReasoning: string;
  runtimeActivities: ToolActivity[];
  pendingTimeline: TurnTimelineItem[];
}

export function toRobotMessages(options: RobotMessageOptions): ChatRobotMessage[] {
  const toolResultsById = indexToolResults(options.messages);
  const msgs = buildSettledRobotMessages(options.messages, toolResultsById);

  if (options.streaming && (options.hasActiveThread || msgs.length > 0)) {
    const pendingTimeline =
      options.pendingTimeline.length > 0
        ? options.pendingTimeline
        : buildPendingTimelineItems(options.pendingAssistantReasoning, options.runtimeActivities);
    const hasVisiblePendingContent = Boolean(
      options.pendingAssistantContent.trim() || pendingTimeline.length > 0,
    );

    msgs.push({
      id: "pending-assistant",
      role: "assistant",
      content: buildRobotContent(options.pendingAssistantContent || "", pendingTimeline),
      loading: !hasVisiblePendingContent,
      state: buildMessageState({
        thinking: true,
        timelineItems: pendingTimeline,
      }),
    });
  }

  return msgs;
}

export function createBubbleRoles(): Record<string, BubbleRoleConfig> {
  return {
    assistant: {
      placement: "start",
      avatar: h("span", { class: "chat-avatar chat-avatar--assistant" }, "AI"),
    },
    tool: {
      placement: "start",
      avatar: h("span", { class: "chat-avatar chat-avatar--tool" }, "T"),
    },
    user: {
      placement: "end",
      avatar: h("span", { class: "chat-avatar chat-avatar--user" }, "我"),
    },
  };
}

export function createStarterPrompts(): PromptProps[] {
  return [
    {
      id: "quality-sop",
      label: "质检 SOP",
      description: "输入环境 + SOP 单号",
      icon: h("span", { class: "prompt-icon" }, "SOP"),
    },
  ];
}

export function draftMessageForPrompt(promptId: string | number | undefined) {
  if (promptId === "quality-sop") {
    return "请根据以下信息执行质检 SOP：环境：；SOP 单号：。";
  }
  return "请根据以下信息执行质检 SOP：环境：；SOP 单号：。";
}

function displayMessageText(
  message: ChatMessageRecord,
  timelineItems: TurnTimelineItem[],
) {
  const content = message.content?.trim();
  if (content) return message.content;

  if (message.role === "assistant") {
    if (timelineItems.length > 0) return "";
    const names = toolCallNames(message.tool_calls);
    if (names.length > 0) return "";
    if (message.reasoning_content?.trim()) return "助手正在思考，等待可见回复。";
  }

  if (message.role === "tool" && message.name?.trim()) {
    return `工具 ${message.name} 返回为空。`;
  }

  if (message.role === "tool") return "工具调用结果为空。";
  return "消息内容为空。";
}

function buildRobotContent(
  text: string,
  timelineItems: TurnTimelineItem[],
): ChatMessageContent {
  const normalizedText = text.trim();

  if (timelineItems.length === 0) {
    return normalizedText ? text : "";
  }

  const content: ChatRobotContentItem[] = [
    {
      type: TURN_TIMELINE_CONTENT_TYPE,
      items: timelineItems,
    },
  ];

  if (normalizedText) {
    content.push({
      type: "text",
      text,
    });
  }

  return content;
}

function buildMessageState(options: {
  thinking: boolean;
  timelineItems: TurnTimelineItem[];
}) {
  const state: ChatRobotMessageState = {};

  if (options.timelineItems.some((item) => item.type === "reasoning")) {
    state.thinking = options.thinking;
    state.open = true;
  }

  const toolDetails = options.timelineItems.filter(
    (item): item is TurnTimelineItem & { type: "tool_call" } =>
      item.type === "tool_call",
  );
  if (toolDetails.length > 0) {
    state.toolDetails = toolDetails;
  }

  return Object.keys(state).length > 0 ? state : undefined;
}

function toolCallNames(toolCalls: unknown[] | null | undefined): string[] {
  if (!Array.isArray(toolCalls)) return [];
  return toolCalls
    .map((toolCall) => {
      if (!toolCall || typeof toolCall !== "object" || !("name" in toolCall)) return "";
      const name = (toolCall as { name?: unknown }).name;
      return typeof name === "string" ? name.trim() : "";
    })
    .filter((name) => name.length > 0);
}

function indexToolResults(messages: ChatMessageRecord[]) {
  const results = new Map<string, ChatMessageRecord>();
  for (const message of messages) {
    if (message.role !== "tool") continue;
    if (!message.tool_call_id?.trim()) continue;
    results.set(message.tool_call_id, message);
  }
  return results;
}

function buildSettledRobotMessages(
  messages: ChatMessageRecord[],
  toolResultsById: Map<string, ChatMessageRecord>,
): ChatRobotMessage[] {
  const robotMessages: ChatRobotMessage[] = [];

  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index];
    if (message.role === "system") continue;

    if (message.role === "assistant") {
      const groupStart = index;
      const turnMessages: ChatMessageRecord[] = [];

      while (
        index < messages.length &&
        (messages[index].role === "assistant" || messages[index].role === "tool")
      ) {
        turnMessages.push(messages[index]);
        index += 1;
      }
      index -= 1;

      robotMessages.push(buildAssistantTurnMessage(turnMessages, groupStart, toolResultsById));
      continue;
    }

    if (message.role === "tool" && message.tool_call_id?.trim()) continue;

    const timelineItems: TurnTimelineItem[] = [];
    robotMessages.push({
      id: settledMessageId(message, index),
      role: message.role,
      content: buildRobotContent(displayMessageText(message, timelineItems), timelineItems),
    });
  }

  return robotMessages;
}

function buildAssistantTurnMessage(
  messages: ChatMessageRecord[],
  startIndex: number,
  toolResultsById: Map<string, ChatMessageRecord>,
): ChatRobotMessage {
  const timelineItems: TurnTimelineItem[] = [];
  let finalText = "";

  for (const message of messages) {
    if (message.role !== "assistant") continue;

    if (message.content?.trim()) {
      finalText = message.content;
    }

    if (message.reasoning_content?.trim()) {
      timelineItems.push({
        type: "reasoning",
        id: `reasoning-${startIndex}-${timelineItems.length}`,
        text: message.reasoning_content,
      });
    }

    for (const toolCall of normalizeToolCalls(message.tool_calls)) {
      const resultMessage = toolCall.id ? toolResultsById.get(toolCall.id) : undefined;
      timelineItems.push({
        type: "tool_call",
        id: toolCall.id || `tool-call-${toolCall.name}-${timelineItems.length}`,
        kind: toolKind(toolCall.name),
        name: toolCall.name,
        status: resultMessage ? "success" : "running",
        inputPreview: previewValue(toolCall.arguments),
        outputPreview: resultMessage?.content ?? "",
      });
    }
  }

  return {
    id: `message-assistant-${startIndex}`,
    role: "assistant",
    content: buildRobotContent(
      timelineItems.length > 0
        ? finalText
        : displayMessageText(messages[messages.length - 1], timelineItems),
      timelineItems,
    ),
    state: buildMessageState({
      thinking: false,
      timelineItems,
    }),
  };
}

function buildPendingTimelineItems(
  pendingAssistantReasoning: string,
  runtimeActivities: ToolActivity[],
): TurnTimelineItem[] {
  const items: TurnTimelineItem[] = [];
  if (pendingAssistantReasoning.trim()) {
    items.push({
      type: "reasoning",
      id: "pending-reasoning",
      text: pendingAssistantReasoning,
    });
  }

  items.push(
    ...runtimeActivities.map((activity) => ({
      type: "tool_call" as const,
      id: activity.id,
      kind: activity.kind === "job" ? ("job" as const) : toolKind(activity.name),
      name: activity.name,
      status: activity.status,
      inputPreview: activity.argumentsPreview,
      outputPreview: activity.resultPreview,
    })),
  );

  return items;
}

function normalizeToolCalls(toolCalls: unknown[] | null | undefined): Array<{
  id: string;
  name: string;
  arguments: unknown;
}> {
  if (!Array.isArray(toolCalls)) return [];

  return toolCalls
    .map((toolCall) => {
      if (!toolCall || typeof toolCall !== "object") return null;
      const idValue = "id" in toolCall ? (toolCall as { id?: unknown }).id : "";
      const nameValue = "name" in toolCall ? (toolCall as { name?: unknown }).name : "";
      const argumentsValue = "arguments" in toolCall
        ? (toolCall as { arguments?: unknown }).arguments
        : undefined;

      const name = typeof nameValue === "string" ? nameValue.trim() : "";
      if (!name) return null;

      return {
        id: typeof idValue === "string" ? idValue.trim() : "",
        name,
        arguments: argumentsValue,
      };
    })
    .filter((toolCall): toolCall is { id: string; name: string; arguments: unknown } => Boolean(toolCall));
}

function previewValue(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function settledMessageId(message: ChatMessageRecord, index: number) {
  if (message.tool_call_id?.trim()) return `tool-call-${message.tool_call_id}`;
  if (message.role === "tool" && message.name?.trim()) return `tool-${message.name}-${index}`;
  return `message-${message.role}-${index}`;
}

function toolKind(name: string): ToolCallDetail["kind"] {
  if (name === "shell" || name.startsWith("shell.") || name === "exec") return "shell";
  if (name.startsWith("mcp.")) return "mcp";
  if (name.includes("search")) return "search";
  if (name.includes("http") || name.includes("fetch")) return "http";
  if (name.includes("file") || name.includes("fs")) return "file";
  return "tool";
}
