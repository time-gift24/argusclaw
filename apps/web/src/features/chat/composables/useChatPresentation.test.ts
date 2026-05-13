import { describe, expect, it } from "vitest";

import type { ChatMessageRecord } from "@/lib/api";
import {
  TURN_TIMELINE_CONTENT_TYPE,
  createStarterPrompts,
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
      pendingTimeline: [],
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
      pendingTimeline: [],
    });

    expect(messages[1]).toMatchObject({
      id: "pending-assistant",
      role: "assistant",
      content: "",
      loading: true,
    });
  });

  it("renders assistant reasoning as ordered timeline items for settled and streaming messages", () => {
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
      pendingTimeline: [],
    });

    expect(messages).toHaveLength(2);
    expect(messages[0]).toMatchObject({
      id: "message-assistant-0",
      role: "assistant",
    });
    expect(messages[0].content).toEqual([
      {
        type: TURN_TIMELINE_CONTENT_TYPE,
        items: [
          {
            type: "reasoning",
            id: "reasoning-0-0",
            text: "先分析上下文，再组织回答。",
          },
        ],
      },
      {
        type: "text",
        text: "最终答案",
      },
    ]);
    expect(messages[1]).toMatchObject({
      role: "assistant",
    });
    expect(messages[1].content).toEqual([
      {
        type: TURN_TIMELINE_CONTENT_TYPE,
        items: [
          {
            type: "reasoning",
            id: "pending-reasoning",
            text: "正在推理当前问题。",
          },
        ],
      },
      {
        type: "text",
        text: "流式回复",
      },
    ]);
  });

  it("renders empty assistant tool-call messages as inline turn timeline items", () => {
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
      pendingTimeline: [],
    });

    expect(messages[0].content).toEqual([
      {
        type: TURN_TIMELINE_CONTENT_TYPE,
        items: [
          {
            type: "tool_call",
            id: "tool-call-shell-0",
            kind: "shell",
            name: "shell",
            status: "running",
            inputPreview: "",
            outputPreview: "",
          },
          {
            type: "tool_call",
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
  });

  it("merges assistant tool calls with matching tool results into an inline timeline", () => {
    const messages = toRobotMessages({
      messages: [
        message("assistant", "", {
          reasoning_content: "先查看目录。",
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
      pendingTimeline: [],
    });

    expect(messages).toHaveLength(1);
    expect(messages[0].role).toBe("assistant");
    expect(messages[0].content).toEqual([
      {
        type: TURN_TIMELINE_CONTENT_TYPE,
        items: [
          {
            type: "reasoning",
            id: "reasoning-0-0",
            text: "先查看目录。",
          },
          {
            type: "tool_call",
            id: "call-shell",
            kind: "shell",
            name: "shell",
            status: "success",
            inputPreview: "{\n  \"cmd\": \"pwd\"\n}",
            outputPreview: "/workspace/project",
          },
          {
            type: "tool_call",
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
  });

  it("preserves settled assistant tool cycles inside one ordered turn timeline", () => {
    const messages = toRobotMessages({
      messages: [
        message("user", "做检查"),
        message("assistant", "", {
          reasoning_content: "先搜信息。",
          tool_calls: [{ id: "call-search", name: "mcp.search", arguments: { q: "runtime" } }],
        }),
        message("tool", "{\"hits\":2}", {
          tool_call_id: "call-search",
          name: "mcp.search",
        }),
        message("assistant", "", {
          reasoning_content: "再看文件。",
          tool_calls: [{ id: "call-file", name: "file.read", arguments: { path: "README.md" } }],
        }),
        message("tool", "README", {
          tool_call_id: "call-file",
          name: "file.read",
        }),
        message("assistant", "完成。"),
      ],
      streaming: false,
      hasActiveThread: true,
      pendingAssistantContent: "",
      pendingAssistantReasoning: "",
      runtimeActivities: [],
      pendingTimeline: [],
    });

    expect(messages).toHaveLength(2);
    expect(messages[1].content).toEqual([
      {
        type: TURN_TIMELINE_CONTENT_TYPE,
        items: [
          expect.objectContaining({ type: "reasoning", text: "先搜信息。" }),
          expect.objectContaining({ type: "tool_call", id: "call-search", status: "success" }),
          expect.objectContaining({ type: "reasoning", text: "再看文件。" }),
          expect.objectContaining({ type: "tool_call", id: "call-file", status: "success" }),
        ],
      },
      {
        type: "text",
        text: "完成。",
      },
    ]);
  });

  it("exposes streaming runtime activities as inline timeline items before text arrives", () => {
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
      pendingTimeline: [
        {
          type: "tool_call",
          id: "call-shell",
          kind: "shell",
          name: "shell",
          status: "running",
          inputPreview: "{\n  \"cmd\": \"pwd\"\n}",
          outputPreview: "",
        },
      ],
    });

    expect(messages[1].loading).toBe(false);
    expect(messages[1].content).toEqual([
      {
        type: TURN_TIMELINE_CONTENT_TYPE,
        items: [
          {
            type: "tool_call",
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
  });

  it("keeps background jobs distinct from normal tool calls in the pending bubble", () => {
    const messages = toRobotMessages({
      messages: [],
      streaming: true,
      hasActiveThread: true,
      pendingAssistantContent: "",
      pendingAssistantReasoning: "",
      runtimeActivities: [
        {
          id: "job-42",
          kind: "job",
          name: "后台 Job job-42",
          status: "running",
          argumentsPreview: "正在执行后台任务",
          resultPreview: "",
        },
      ],
      pendingTimeline: [
        {
          type: "tool_call",
          id: "job-42",
          kind: "job",
          name: "后台 Job job-42",
          status: "running",
          inputPreview: "正在执行后台任务",
          outputPreview: "",
        },
      ],
    });

    expect(messages[0].content).toEqual([
      {
        type: TURN_TIMELINE_CONTENT_TYPE,
        items: [
          expect.objectContaining({
            type: "tool_call",
            id: "job-42",
            kind: "job",
            name: "后台 Job job-42",
          }),
        ],
      },
    ]);
  });

  it("creates a single quality SOP starter prompt", () => {
    const prompts = createStarterPrompts();

    expect(prompts).toHaveLength(1);
    expect(prompts[0]).toMatchObject({
      id: "quality-sop",
      label: "质检 SOP",
      description: "输入环境 + SOP 单号",
    });
  });

  it("maps starter prompt ids to the quality SOP draft message", () => {
    expect(draftMessageForPrompt("quality-sop")).toContain("质检 SOP");
    expect(draftMessageForPrompt("quality-sop")).toContain("环境：");
    expect(draftMessageForPrompt("quality-sop")).toContain("SOP 单号：");
    expect(draftMessageForPrompt("unknown")).toBe(draftMessageForPrompt("quality-sop"));
  });
});
