import { mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", () => ({
  TinyButton: defineComponent({
    name: "TinyButton",
    emits: ["click"],
    setup(_, { emit, slots }) {
      return () =>
        h(
          "button",
          {
            type: "button",
            onClick: () => emit("click"),
          },
          slots.default?.(),
        );
    },
  }),
  TinyInput: defineComponent({
    name: "TinyInput",
    props: {
      modelValue: {
        type: String,
        default: "",
      },
    },
    emits: ["update:modelValue"],
    setup(props, { emit }) {
      return () =>
        h("input", {
          value: props.modelValue,
          onInput: (event: Event) => emit("update:modelValue", (event.target as HTMLInputElement).value),
        });
    },
  }),
}));

vi.mock("@/lib/api", () => ({
  getApiClient: () => ({
    listChatThreads: vi.fn().mockResolvedValue([]),
  }),
}));

import ChatHistoryDialog from "./ChatHistoryDialog.vue";

afterEach(() => {
  document.body.innerHTML = "";
});

describe("ChatHistoryDialog", () => {
  it("emits deleteSession from the inline confirmation even when TinyButton does not pass a native event", async () => {
    const wrapper = mount(ChatHistoryDialog, {
      attachTo: document.body,
      props: {
        modelValue: true,
        sessions: [
          {
            id: "session-1",
            name: "旧会话",
            thread_count: 1,
            updated_at: "2026-04-24T10:00:00Z",
          },
        ],
        activeSessionId: "session-1",
        activeThreadId: "thread-1",
        sessionListLoading: false,
      },
    });

    const deleteButton = document.querySelector(".history-dialog__action-btn--danger") as HTMLButtonElement | null;
    expect(deleteButton).toBeDefined();
    deleteButton!.click();
    await wrapper.vm.$nextTick();

    const confirmButton = document.querySelector("[data-testid='confirm-delete-session']") as HTMLButtonElement | null;
    expect(confirmButton).toBeDefined();
    confirmButton!.click();
    await wrapper.vm.$nextTick();

    expect(wrapper.emitted("deleteSession")).toEqual([["session-1"]]);
  });
});
