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

    expect(messages).toEqual([
      { role: "user", content: "你好" },
      { role: "assistant", content: "正在回答" },
    ]);
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
