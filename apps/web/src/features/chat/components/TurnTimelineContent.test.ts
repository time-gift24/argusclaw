import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import { TURN_TIMELINE_CONTENT_TYPE, type ChatRobotMessage } from "../composables/useChatPresentation";
import TurnTimelineContent from "./TurnTimelineContent.vue";

function message(items: ChatRobotMessage["content"]): ChatRobotMessage {
  return {
    role: "assistant",
    content: items,
  };
}

describe("TurnTimelineContent", () => {
  it("groups consecutive tool calls into one collapsible timeline block", () => {
    const wrapper = mount(TurnTimelineContent, {
      props: {
        contentIndex: 0,
        message: message([
          {
            type: TURN_TIMELINE_CONTENT_TYPE,
            items: [
              {
                type: "tool_call",
                id: "glob-html",
                kind: "search",
                name: "Glob",
                status: "success",
                inputPreview: "**/*.html",
                outputPreview: "index.html",
              },
              {
                type: "tool_call",
                id: "glob-md",
                kind: "search",
                name: "Glob",
                status: "success",
                inputPreview: "**/*.md",
                outputPreview: "README.md",
              },
              {
                type: "tool_call",
                id: "glob-all",
                kind: "search",
                name: "Glob",
                status: "success",
                inputPreview: "**/*",
                outputPreview: "3 files",
              },
            ],
          },
        ]),
      },
    });

    const group = wrapper.get(".turn-timeline__tool-group");
    expect(group.attributes("open")).toBeUndefined();
    expect(group.get(".turn-timeline__tool-group-summary").text()).toContain("检索 ×3，完成");
    expect(wrapper.findAll(".turn-timeline__tool")).toHaveLength(3);
    expect(wrapper.text()).toContain("**/*.html");
    expect(wrapper.text()).toContain("**/*.md");
    expect(wrapper.text()).toContain("**/*");
  });

  it("starts a new tool group after a reasoning item", () => {
    const wrapper = mount(TurnTimelineContent, {
      props: {
        contentIndex: 0,
        message: message([
          {
            type: TURN_TIMELINE_CONTENT_TYPE,
            items: [
              {
                type: "tool_call",
                id: "search-1",
                kind: "search",
                name: "Search",
                status: "success",
                inputPreview: "alpha",
                outputPreview: "done",
              },
              {
                type: "tool_call",
                id: "search-2",
                kind: "search",
                name: "Search",
                status: "success",
                inputPreview: "beta",
                outputPreview: "done",
              },
              {
                type: "reasoning",
                id: "reasoning-1",
                text: "再换一类查询。",
              },
              {
                type: "tool_call",
                id: "file-1",
                kind: "file",
                name: "Read",
                status: "running",
                inputPreview: "README.md",
                outputPreview: "",
              },
              {
                type: "tool_call",
                id: "file-2",
                kind: "file",
                name: "Read",
                status: "running",
                inputPreview: "DESIGN.md",
                outputPreview: "",
              },
            ],
          },
        ]),
      },
    });

    const groups = wrapper.findAll(".turn-timeline__tool-group");
    expect(groups).toHaveLength(2);
    expect(groups[0].text()).toContain("检索 ×2，完成");
    expect(groups[1].text()).toContain("文件操作 ×2，运行中");
  });
});
