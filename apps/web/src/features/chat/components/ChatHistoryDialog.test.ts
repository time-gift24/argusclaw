import { mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";
import { readFileSync } from "node:fs";
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

vi.mock("@opentiny/tiny-robot-svgs/dist/tiny-robot-svgs.js", () => ({
  IconDelete: defineComponent({
    name: "IconDelete",
    setup() {
      return () => h("svg", { "data-testid": "tiny-icon-delete" });
    },
  }),
  IconEditPen: defineComponent({
    name: "IconEditPen",
    setup() {
      return () => h("svg", { "data-testid": "tiny-icon-edit" });
    },
  }),
}));

import ChatHistoryDialog from "./ChatHistoryDialog.vue";

afterEach(() => {
  document.body.innerHTML = "";
});

describe("ChatHistoryDialog", () => {
  it("uses Tiny icons for rename and delete action buttons", async () => {
    mount(ChatHistoryDialog, {
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

    expect(document.querySelector("[data-testid='tiny-icon-edit']")).toBeTruthy();
    expect(document.querySelector("[data-testid='tiny-icon-delete']")).toBeTruthy();
    expect(document.querySelector(".history-dialog__item-actions")?.textContent ?? "").not.toContain("✎");
    expect(document.querySelector(".history-dialog__item-actions")?.textContent ?? "").not.toContain("✕");
  });

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

  it("anchors the delete confirmation actions to the right side of the session row", () => {
    const source = readFileSync("src/features/chat/components/ChatHistoryDialog.vue", "utf8");

    expect(source).toContain(".history-dialog__inline-actions {");
    expect(source).toContain("padding-right: 128px;");
    expect(source).toContain("position: absolute;");
    expect(source).toContain("right: var(--space-2, 8px);");
    expect(source).not.toContain("margin-top: var(--space-2, 8px);");
  });
});
