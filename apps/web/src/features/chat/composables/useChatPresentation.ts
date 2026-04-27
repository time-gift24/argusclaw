import { h } from "vue";
import type { BubbleRoleConfig, PromptProps } from "@opentiny/tiny-robot";

import type { ChatMessageRecord } from "@/lib/api";

export interface ChatRobotMessage {
  id?: string;
  role: string;
  content: string;
  reasoning_content?: string;
  loading?: boolean;
  state?: Record<string, unknown>;
}

export interface RobotMessageOptions {
  messages: ChatMessageRecord[];
  streaming: boolean;
  hasActiveThread: boolean;
  pendingAssistantContent: string;
  pendingAssistantReasoning: string;
}

export function toRobotMessages(options: RobotMessageOptions): ChatRobotMessage[] {
  const msgs: ChatRobotMessage[] = options.messages
    .filter((message) => message.role !== "system")
    .map((message, index): ChatRobotMessage => ({
      id: settledMessageId(message, index),
      role: message.role,
      content: displayMessageContent(message),
      reasoning_content: message.reasoning_content?.trim() ? message.reasoning_content : undefined,
      state: buildReasoningState(message.reasoning_content, false),
    }));

  if (options.streaming && (options.hasActiveThread || msgs.length > 0)) {
    const hasVisiblePendingContent = Boolean(
      options.pendingAssistantContent.trim() || options.pendingAssistantReasoning.trim(),
    );

    msgs.push({
      id: "pending-assistant",
      role: "assistant",
      content: options.pendingAssistantContent || "",
      reasoning_content: options.pendingAssistantReasoning || undefined,
      loading: !hasVisiblePendingContent,
      state: buildReasoningState(options.pendingAssistantReasoning, true),
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
      id: "provider",
      label: "检查模型配置",
      description: "当前默认模型和可用 provider 是否适合这个任务？",
      icon: h("span", { class: "prompt-icon" }, "AI"),
    },
    {
      id: "mcp",
      label: "规划 MCP 运维",
      description: "帮我整理当前 MCP 服务的风险和下一步动作。",
      icon: h("span", { class: "prompt-icon" }, "MCP"),
    },
    {
      id: "template",
      label: "优化智能体模板",
      description: "基于当前模板给出系统提示词改进建议。",
      icon: h("span", { class: "prompt-icon" }, "TPL"),
    },
  ];
}

export function draftMessageForPrompt(promptId: string | number | undefined) {
  if (promptId === "provider") {
    return "请检查当前默认模型、提供方和智能体模板是否适合继续这个任务。";
  }
  if (promptId === "mcp") {
    return "请帮我梳理当前 MCP 服务的运行风险、可用工具和下一步运维动作。";
  }
  return "请基于当前智能体模板，给出系统提示词和工具配置的改进建议。";
}

function displayMessageContent(message: ChatMessageRecord) {
  const content = message.content?.trim();
  if (content) return message.content;

  if (message.role === "assistant") {
    const names = toolCallNames(message.tool_calls);
    if (names.length > 0) return `正在调用工具：${names.join("、")}`;
    if (message.reasoning_content?.trim()) return "助手正在思考，等待可见回复。";
  }

  if (message.role === "tool" && message.name?.trim()) {
    return `工具 ${message.name} 返回为空。`;
  }

  if (message.role === "tool") return "工具调用结果为空。";
  return "消息内容为空。";
}

function buildReasoningState(
  reasoningContent: string | null | undefined,
  thinking: boolean,
) {
  if (!reasoningContent?.trim()) return undefined;

  return {
    thinking,
    open: true,
  };
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
