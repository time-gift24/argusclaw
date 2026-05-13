import { mount } from "@vue/test-utils";
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
  it("does not own vertical autoscroll because the chat page is the scroll container", async () => {
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
    stage.scrollTo = scrollTo;

    await wrapper.setProps({ messages: createMessages(3) });

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

  it("lets assistant output fill the responsive chat body", () => {
    const source = readFileSync("src/features/chat/components/ChatMessageStage.vue", "utf8");

    expect(source).toContain("flex: 1 0 auto;");
    expect(source).not.toMatch(/(^|\n)\s*flex: 1;/);
    expect(source).toContain("--assistant-readable-width: 100%;");
    expect(source).toContain("padding: var(--space-2) 0 calc(var(--chat-dock-clearance, 132px) + var(--space-5));");
    expect(source).toContain("position: relative;");
    expect(source).toContain("left: calc(0px - 44px);");
    expect(source).not.toContain("var(--chat-message-width");
    expect(source).not.toContain("--assistant-readable-width: 860px;");
    expect(source).not.toContain("calc(var(--assistant-readable-width) + 44px)");
  });

  it("keeps vertical scroll ownership on the chat page instead of nested message scrollers", () => {
    const stageSource = readFileSync("src/features/chat/components/ChatMessageStage.vue", "utf8");
    const timelineSource = readFileSync("src/features/chat/components/TurnTimelineContent.vue", "utf8");

    expect(stageSource).not.toContain("auto-scroll");
    expect(stageSource).not.toContain("scrollStageToBottom");
    expect(stageSource).not.toMatch(/\.message-stage\s*\{[^}]*overscroll-behavior:\s*contain;/);
    expect(stageSource).toContain(":deep(.tr-bubble-list)");
    expect(stageSource).toContain("overflow-y: visible");
    expect(stageSource).toContain("overflow: visible !important;");
    expect(timelineSource).not.toContain("max-height: 220px");
    expect(timelineSource).not.toMatch(/overflow:\s*auto/);
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

  it("uses design tokens for markdown table header backgrounds", () => {
    const source = readFileSync("src/features/chat/components/ChatMessageStage.vue", "utf8");

    expect(source).toContain(".tr-bubble__markdown th");
    expect(source).toContain("background: color-mix(in srgb, var(--surface-overlay) 72%, var(--surface-raised));");
    expect(source).toContain("color: var(--text-primary);");
  });

  it("lets markdown tables size to their content instead of filling the chat body", () => {
    const source = readFileSync("src/features/chat/components/ChatMessageStage.vue", "utf8");
    const tableBlock = source.match(/\.tr-bubble__markdown table\)[\s\S]*?\n}/)?.[0] ?? "";

    expect(tableBlock).toContain("display: inline-table;");
    expect(tableBlock).toContain("width: auto;");
    expect(tableBlock).toContain("max-width: 100%;");
    expect(source).toContain("overflow-x: auto;");
    expect(tableBlock).not.toMatch(/(^|\n)\s*width:\s*100%;/);
  });

  it("uses visible rounded borders for markdown tables", () => {
    const source = readFileSync("src/features/chat/components/ChatMessageStage.vue", "utf8");
    const tableBlock = source.match(/\.tr-bubble__markdown table\)[\s\S]*?\n}/)?.[0] ?? "";

    expect(tableBlock).toContain("overflow: hidden;");
    expect(tableBlock).toContain("border-collapse: separate;");
    expect(tableBlock).toContain("border-spacing: 0;");
    expect(tableBlock).toContain("border: 1px solid var(--border-default);");
    expect(tableBlock).toContain("border-radius: 10px;");
    expect(source).toContain("border-right: 1px solid var(--border-default);");
    expect(source).toContain("border-bottom: 1px solid var(--border-default);");
  });
});
