import { flushPromises, mount } from "@vue/test-utils";
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
});
