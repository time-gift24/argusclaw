import { h } from "vue";
import type {
  BubbleRoleConfig,
  ChatMessageContent,
  ChatMessageContentItem,
  PromptProps,
} from "@opentiny/tiny-robot";

import type { ChatMessageRecord } from "@/lib/api";
import type { ToolActivity, ToolActivityStatus } from "./useChatThreadStream";

export const TOOL_SUMMARY_CONTENT_TYPE = "argus-tool-summary";

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

export interface TextContentItem extends ChatMessageContentItem {
  type: "text";
  text: string;
}

export type ChatRobotContentItem = ToolSummaryContentItem | TextContentItem;

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
}

export function toRobotMessages(options: RobotMessageOptions): ChatRobotMessage[] {
  const assistantToolCallIds = collectAssistantToolCallIds(options.messages);
  const toolResultsById = indexToolResults(options.messages);
  const msgs: ChatRobotMessage[] = options.messages
    .filter((message) => shouldRenderMessage(message, assistantToolCallIds))
    .map((message, index): ChatRobotMessage => {
      const toolDetails = buildMessageToolDetails(message, toolResultsById);
      const content = buildRobotContent(displayMessageText(message, toolDetails), toolDetails);

      return {
        id: settledMessageId(message, index),
        role: message.role,
        content,
        reasoning_content: message.reasoning_content?.trim() ? message.reasoning_content : undefined,
        state: buildMessageState({
          reasoningContent: message.reasoning_content,
          thinking: false,
          toolDetails,
        }),
      };
    });

  if (options.streaming && (options.hasActiveThread || msgs.length > 0)) {
    const pendingToolDetails = buildPendingToolDetails(options.runtimeActivities);
    const hasVisiblePendingContent = Boolean(
      options.pendingAssistantContent.trim()
        || options.pendingAssistantReasoning.trim()
        || pendingToolDetails.length > 0,
    );

    msgs.push({
      id: "pending-assistant",
      role: "assistant",
      content: buildRobotContent(options.pendingAssistantContent || "", pendingToolDetails),
      reasoning_content: options.pendingAssistantReasoning || undefined,
      loading: !hasVisiblePendingContent,
      state: buildMessageState({
        reasoningContent: options.pendingAssistantReasoning,
        thinking: true,
        toolDetails: pendingToolDetails,
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
  toolDetails: ToolCallDetail[],
) {
  const content = message.content?.trim();
  if (content) return message.content;

  if (message.role === "assistant") {
    if (toolDetails.length > 0) return "";
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

function buildRobotContent(text: string, toolDetails: ToolCallDetail[]): ChatMessageContent {
  const normalizedText = text.trim();

  if (toolDetails.length === 0) {
    return normalizedText ? text : "";
  }

  const content: ChatRobotContentItem[] = [
    {
      type: TOOL_SUMMARY_CONTENT_TYPE,
      toolDetails,
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
  reasoningContent: string | null | undefined;
  thinking: boolean;
  toolDetails: ToolCallDetail[];
}) {
  const state: ChatRobotMessageState = {};

  if (options.reasoningContent?.trim()) {
    state.thinking = options.thinking;
    state.open = true;
  }

  if (options.toolDetails.length > 0) {
    state.toolDetails = options.toolDetails;
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

function settledMessageId(message: ChatMessageRecord, index: number) {
  if (message.tool_call_id?.trim()) return `tool-call-${message.tool_call_id}`;
  if (message.role === "tool" && message.name?.trim()) return `tool-${message.name}-${index}`;
  return `message-${message.role}-${index}`;
}

function shouldRenderMessage(message: ChatMessageRecord, assistantToolCallIds: Set<string>) {
  if (message.role === "system") return false;
  if (message.role !== "tool") return true;
  if (!message.tool_call_id?.trim()) return true;
  return !assistantToolCallIds.has(message.tool_call_id);
}

function collectAssistantToolCallIds(messages: ChatMessageRecord[]) {
  const ids = new Set<string>();
  for (const message of messages) {
    if (message.role !== "assistant") continue;
    for (const toolCall of normalizeToolCalls(message.tool_calls)) {
      if (toolCall.id) ids.add(toolCall.id);
    }
  }
  return ids;
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

function buildMessageToolDetails(
  message: ChatMessageRecord,
  toolResultsById: Map<string, ChatMessageRecord>,
): ToolCallDetail[] {
  if (message.role !== "assistant") return [];

  return normalizeToolCalls(message.tool_calls).map((toolCall, index) => {
    const resultMessage = toolCall.id ? toolResultsById.get(toolCall.id) : undefined;
    return {
      id: toolCall.id || `tool-call-${toolCall.name}-${index}`,
      kind: toolKind(toolCall.name),
      name: toolCall.name,
      status: resultMessage ? "success" : "running",
      inputPreview: previewValue(toolCall.arguments),
      outputPreview: resultMessage?.content ?? "",
    };
  });
}

function buildPendingToolDetails(runtimeActivities: ToolActivity[]): ToolCallDetail[] {
  return runtimeActivities.map((activity) => ({
    id: activity.id,
    kind: activity.kind === "job" ? "job" : toolKind(activity.name),
    name: activity.name,
    status: activity.status,
    inputPreview: activity.argumentsPreview,
    outputPreview: activity.resultPreview,
  }));
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

function toolKind(name: string): ToolCallDetail["kind"] {
  if (name === "shell" || name.startsWith("shell.") || name === "exec") return "shell";
  if (name.startsWith("mcp.")) return "mcp";
  if (name.includes("search")) return "search";
  if (name.includes("http") || name.includes("fetch")) return "http";
  if (name.includes("file") || name.includes("fs")) return "file";
  return "tool";
}
