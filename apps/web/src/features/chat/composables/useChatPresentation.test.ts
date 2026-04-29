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
      runtimeActivities: [],
    });

    expect(messages).toHaveLength(2);
    expect(messages[0]).toMatchObject({ role: "user", content: "你好" });
    expect(messages[1]).toMatchObject({
      id: "pending-assistant",
      role: "assistant",
      content: "正在回答",
      loading: false,
    });
  });

  it("uses loading chrome only before streamed content becomes visible", () => {
    const messages = toRobotMessages({
      messages: [message("user", "先别急")],
      streaming: true,
      hasActiveThread: true,
      pendingAssistantContent: "",
      pendingAssistantReasoning: "",
      runtimeActivities: [],
    });

    expect(messages[1]).toMatchObject({
      id: "pending-assistant",
      role: "assistant",
      content: "",
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
      runtimeActivities: [],
    });

    expect(messages).toHaveLength(2);
    expect(messages[0]).toMatchObject({
      id: "message-assistant-0",
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
      runtimeActivities: [],
    });

    expect(messages[0].content).toEqual([
      {
        type: "argus-tool-summary",
        toolDetails: [
          {
            id: "tool-call-shell-0",
            kind: "shell",
            name: "shell",
            status: "running",
            inputPreview: "",
            outputPreview: "",
          },
          {
            id: "tool-call-mcp.search-1",
            kind: "mcp",
            name: "mcp.search",
            status: "running",
            inputPreview: "",
            outputPreview: "",
          },
        ],
      },
    ]);
    expect(messages).toHaveLength(1);
    expect((messages[0].state as { toolDetails?: unknown[] } | undefined)?.toolDetails).toEqual([
      {
        id: "tool-call-shell-0",
        kind: "shell",
        name: "shell",
        status: "running",
        inputPreview: "",
        outputPreview: "",
      },
      {
        id: "tool-call-mcp.search-1",
        kind: "mcp",
        name: "mcp.search",
        status: "running",
        inputPreview: "",
        outputPreview: "",
      },
    ]);
  });

  it("merges assistant tool calls with matching tool results into a single dialog-ready summary", () => {
    const messages = toRobotMessages({
      messages: [
        message("assistant", "", {
          tool_calls: [
            { id: "call-shell", name: "shell", arguments: { cmd: "pwd" } },
            { id: "call-search", name: "mcp.search", arguments: { q: "runtime" } },
          ],
        }),
        message("tool", "/workspace/project", {
          tool_call_id: "call-shell",
          name: "shell",
        }),
        message("tool", "{\"hits\":2}", {
          tool_call_id: "call-search",
          name: "mcp.search",
        }),
      ],
      streaming: false,
      hasActiveThread: true,
      pendingAssistantContent: "",
      pendingAssistantReasoning: "",
      runtimeActivities: [],
    });

    expect(messages).toHaveLength(1);
    expect(messages[0].role).toBe("assistant");
    expect(messages[0].content).toEqual([
      {
        type: "argus-tool-summary",
        toolDetails: [
          {
            id: "call-shell",
            kind: "shell",
            name: "shell",
            status: "success",
            inputPreview: "{\n  \"cmd\": \"pwd\"\n}",
            outputPreview: "/workspace/project",
          },
          {
            id: "call-search",
            kind: "mcp",
            name: "mcp.search",
            status: "success",
            inputPreview: "{\n  \"q\": \"runtime\"\n}",
            outputPreview: "{\"hits\":2}",
          },
        ],
      },
    ]);
    expect((messages[0].state as { toolDetails?: unknown[] } | undefined)?.toolDetails).toEqual([
      {
        id: "call-shell",
        kind: "shell",
        name: "shell",
        status: "success",
        inputPreview: "{\n  \"cmd\": \"pwd\"\n}",
        outputPreview: "/workspace/project",
      },
      {
        id: "call-search",
        kind: "mcp",
        name: "mcp.search",
        status: "success",
        inputPreview: "{\n  \"q\": \"runtime\"\n}",
        outputPreview: "{\"hits\":2}",
      },
    ]);
  });

  it("exposes streaming runtime activities as clickable tool summaries before text arrives", () => {
    const messages = toRobotMessages({
      messages: [message("user", "继续")],
      streaming: true,
      hasActiveThread: true,
      pendingAssistantContent: "",
      pendingAssistantReasoning: "",
      runtimeActivities: [
        {
          id: "call-shell",
          name: "shell",
          status: "running",
          argumentsPreview: "{\n  \"cmd\": \"pwd\"\n}",
          resultPreview: "",
        },
      ],
    });

    expect(messages[1].loading).toBe(false);
    expect(messages[1].content).toEqual([
      {
        type: "argus-tool-summary",
        toolDetails: [
          {
            id: "call-shell",
            kind: "shell",
            name: "shell",
            status: "running",
            inputPreview: "{\n  \"cmd\": \"pwd\"\n}",
            outputPreview: "",
          },
        ],
      },
    ]);
    expect((messages[1].state as { toolDetails?: unknown[] } | undefined)?.toolDetails).toEqual([
      {
        id: "call-shell",
        kind: "shell",
        name: "shell",
        status: "running",
        inputPreview: "{\n  \"cmd\": \"pwd\"\n}",
        outputPreview: "",
      },
    ]);
  });

  it("maps starter prompt ids to Chinese draft messages", () => {
    expect(draftMessageForPrompt("provider")).toContain("当前默认模型");
    expect(draftMessageForPrompt("mcp")).toContain("当前 MCP 服务");
    expect(draftMessageForPrompt("unknown")).toContain("当前智能体模板");
  });
});
