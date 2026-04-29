import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it } from "vitest";

import ToolCallSummaryContent from "./ToolCallSummaryContent.vue";

afterEach(() => {
  document.body.innerHTML = "";
});

describe("ToolCallSummaryContent", () => {
  it("opens a detail dialog with tool input and output when a tool row is clicked", async () => {
    const wrapper = mount(ToolCallSummaryContent, {
      attachTo: document.body,
      props: {
        message: {
          role: "assistant",
          content: [
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
              ],
            },
          ],
        },
        contentIndex: 0,
      },
    });

    expect(wrapper.text()).toContain("shell");
    expect(wrapper.text()).not.toContain("\"cmd\": \"pwd\"");
    expect(wrapper.text()).not.toContain("/workspace/project");
    expect(document.body.textContent ?? "").not.toContain("工具详情");

    await wrapper.get("button").trigger("click");

    expect(document.body.textContent ?? "").toContain("工具详情");
    expect(document.body.textContent ?? "").toContain("输入");
    expect(document.body.textContent ?? "").toContain("\"cmd\": \"pwd\"");
    expect(document.body.textContent ?? "").toContain("输出");
    expect(document.body.textContent ?? "").toContain("/workspace/project");

    const closeButton = Array.from(document.body.querySelectorAll("button")).find((button) =>
      button.textContent?.includes("关闭"),
    ) as HTMLButtonElement | undefined;
    expect(closeButton).toBeDefined();
    closeButton!.click();
    await wrapper.vm.$nextTick();

    expect(document.body.textContent ?? "").not.toContain("工具详情");
  });
});
