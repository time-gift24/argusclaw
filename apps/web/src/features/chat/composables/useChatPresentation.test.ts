import { describe, expect, it } from "vitest";

import type { ChatMessageRecord } from "@/lib/api";
import {
  draftMessageForPrompt,
  toRobotMessages,
} from "./useChatPresentation";

function message(
  role: ChatMessageRecord["role"],
  content: string,
  overrides: Partial<ChatMessageRecord> = {},
): ChatMessageRecord {
  return {
    role,
    content,
    reasoning_content: null,
    content_parts: [],
    tool_call_id: null,
    name: null,
    tool_calls: null,
    metadata: null,
    ...overrides,
  };
}

describe("useChatPresentation", () => {
  it("keeps a single pending assistant bubble for streamed content", () => {
    const messages = toRobotMessages({
      messages: [message("user", "你好")],
      streaming: true,
      hasActiveThread: true,
      pendingAssistantContent: "正在回答",
      pendingAssistantReasoning: "",
    });

    expect(messages).toHaveLength(2);
    expect(messages[0]).toMatchObject({ role: "user", content: "你好" });
    expect(messages[1]).toMatchObject({
      role: "assistant",
      content: "正在回答",
      loading: true,
    });
  });

  it("preserves assistant reasoning content for settled and streaming messages", () => {
    const messages = toRobotMessages({
      messages: [
        message("assistant", "最终答案", {
          reasoning_content: "先分析上下文，再组织回答。",
        }),
      ],
      streaming: true,
      hasActiveThread: true,
      pendingAssistantContent: "流式回复",
      pendingAssistantReasoning: "正在推理当前问题。",
    });

    expect(messages).toHaveLength(2);
    expect(messages[0]).toMatchObject({
      role: "assistant",
      content: "最终答案",
      reasoning_content: "先分析上下文，再组织回答。",
    });
    expect(messages[1]).toMatchObject({
      role: "assistant",
      content: "流式回复",
      reasoning_content: "正在推理当前问题。",
    });
  });

  it("renders empty assistant tool-call messages as readable summaries", () => {
    const messages = toRobotMessages({
      messages: [
        message("assistant", "", {
          tool_calls: [{ name: "shell" }, { name: "mcp.search" }],
        }),
      ],
      streaming: false,
      hasActiveThread: true,
      pendingAssistantContent: "",
      pendingAssistantReasoning: "",
    });

    expect(messages[0].content).toBe("正在调用工具：shell、mcp.search");
  });

  it("maps starter prompt ids to Chinese draft messages", () => {
    expect(draftMessageForPrompt("provider")).toContain("当前默认模型");
    expect(draftMessageForPrompt("mcp")).toContain("当前 MCP 服务");
    expect(draftMessageForPrompt("unknown")).toContain("当前智能体模板");
  });
});
