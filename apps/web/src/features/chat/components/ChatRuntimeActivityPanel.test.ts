import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));

import ChatRuntimeActivityPanel from "./ChatRuntimeActivityPanel.vue";

afterEach(() => {
  document.body.innerHTML = "";
});

describe("ChatRuntimeActivityPanel", () => {
  it("keeps tool input and output collapsed until the matching activity is clicked", async () => {
    const wrapper = mount(ChatRuntimeActivityPanel, {
      attachTo: document.body,
      props: {
        notice: "",
        activities: [
          {
            id: "call-shell",
            name: "shell",
            status: "success",
            argumentsPreview: "{\n  \"cmd\": \"pwd\"\n}",
            resultPreview: "/workspace/project",
          },
        ],
      },
    });

    expect(wrapper.text()).toContain("shell");
    expect(wrapper.text()).not.toContain("\"cmd\": \"pwd\"");
    expect(wrapper.text()).not.toContain("/workspace/project");

    await wrapper.get("button").trigger("click");

    expect(document.body.textContent ?? "").toContain("工具详情");
    expect(document.body.textContent ?? "").toContain("\"cmd\": \"pwd\"");
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
