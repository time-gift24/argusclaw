import { flushPromises, mount } from "@vue/test-utils";
import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";

vi.mock("@opentiny/tiny-robot", async () => import("@/test/stubs/tiny-robot"));

import ChatMessageStage from "./ChatMessageStage.vue";

function createMessages(count: number) {
  return Array.from({ length: count }, (_, index) => ({
    role: index % 2 === 0 ? "assistant" : "user",
    content: `message-${index}`,
    reasoning_content: undefined,
  }));
}

describe("ChatMessageStage", () => {
  it("sticks to the bottom when new messages arrive and the user is near the bottom", async () => {
    const wrapper = mount(ChatMessageStage, {
      props: {
        loading: false,
        messages: createMessages(2),
        bubbleRoles: {},
        starterPrompts: [],
      },
    });

    const stage = wrapper.get(".message-stage").element as HTMLDivElement;
    const scrollTo = vi.fn();
    Object.defineProperty(stage, "clientHeight", { configurable: true, value: 240 });
    Object.defineProperty(stage, "scrollHeight", { configurable: true, value: 600 });
    Object.defineProperty(stage, "scrollTop", { configurable: true, writable: true, value: 360 });
    stage.scrollTo = scrollTo;

    await wrapper.setProps({ messages: createMessages(3) });
    await flushPromises();

    expect(scrollTo).toHaveBeenCalledWith({ top: 600, behavior: "auto" });
  });

  it("does not force scroll when the user has scrolled away from the bottom", async () => {
    const wrapper = mount(ChatMessageStage, {
      props: {
        loading: false,
        messages: createMessages(2),
        bubbleRoles: {},
        starterPrompts: [],
      },
    });

    const stage = wrapper.get(".message-stage").element as HTMLDivElement;
    const scrollTo = vi.fn();
    Object.defineProperty(stage, "clientHeight", { configurable: true, value: 240 });
    Object.defineProperty(stage, "scrollHeight", { configurable: true, value: 600 });
    Object.defineProperty(stage, "scrollTop", { configurable: true, writable: true, value: 40 });
    stage.scrollTo = scrollTo;
    await flushPromises();
    scrollTo.mockClear();

    await wrapper.trigger("scroll");
    await wrapper.setProps({ messages: createMessages(3) });
    await flushPromises();

    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("uses the flat single-scroll stage chrome for immersive chat", () => {
    const wrapper = mount(ChatMessageStage, {
      props: {
        loading: false,
        messages: createMessages(2),
        bubbleRoles: {},
        starterPrompts: [],
      },
    });

    expect(wrapper.find(".message-stage--flat").exists()).toBe(true);
    expect(wrapper.find(".message-stage--centered-assistant").exists()).toBe(true);
  });

  it("uses the page chat width for assistant output instead of a narrower local width", () => {
    const source = readFileSync("src/features/chat/components/ChatMessageStage.vue", "utf8");

    expect(source).toContain("flex: 1 0 auto;");
    expect(source).not.toMatch(/(^|\n)\s*flex: 1;/);
    expect(source).toContain("--assistant-readable-width: var(--chat-message-width, 1120px);");
    expect(source).toContain("padding: var(--space-2) 0 calc(var(--chat-dock-clearance, 132px) + var(--space-5));");
    expect(source).toContain("position: relative;");
    expect(source).toContain("left: calc(0px - 44px);");
    expect(source).not.toContain("--assistant-readable-width: 860px;");
    expect(source).not.toContain("calc(var(--assistant-readable-width) + 44px)");
  });

  it("configures TinyRobot markdown rendering for richer assistant output", () => {
    const wrapper = mount(ChatMessageStage, {
      props: {
        loading: false,
        messages: [
          {
            role: "assistant",
            content: "| 列 A | 列 B |\n| --- | --- |\n| 1 | 2 |",
            reasoning_content: undefined,
          },
        ],
        bubbleRoles: {},
        starterPrompts: [],
      },
    });

    const provider = wrapper.get(".tr-bubble-provider-stub");

    expect(provider.attributes("data-fallback-content-renderer")).toBe("set");
    expect(JSON.parse(provider.attributes("data-md-config") ?? "{}")).toMatchObject({
      html: false,
      linkify: true,
      typographer: true,
    });
  });
});
