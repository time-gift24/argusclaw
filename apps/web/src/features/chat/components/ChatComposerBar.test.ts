import { mount } from "@vue/test-utils";
import { readFileSync } from "node:fs";
import { defineComponent, h } from "vue";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));
vi.mock("@opentiny/tiny-robot", async () => import("@/test/stubs/tiny-robot"));
vi.mock("@opentiny/tiny-robot-svgs/dist/tiny-robot-svgs.js", () => ({
  IconAi: defineComponent({
    name: "IconAi",
    setup() {
      return () => h("svg", { "data-testid": "tiny-icon-ai" });
    },
  }),
  IconUser: defineComponent({
    name: "IconUser",
    setup() {
      return () => h("svg", { "data-testid": "tiny-icon-user" });
    },
  }),
}));

import type { AgentRecord, LlmProviderRecord } from "@/lib/api";
import ChatComposerBar from "./ChatComposerBar.vue";

describe("ChatComposerBar", () => {
  function template(overrides: Partial<AgentRecord> = {}): AgentRecord {
    return {
      id: 3,
      display_name: "通用助手",
      description: "适合日常问答。",
      version: "1.0.0",
      provider_id: null,
      model_id: null,
      system_prompt: "You are helpful.",
      tool_names: [],
      subagent_names: [],
      max_tokens: null,
      temperature: null,
      thinking_config: null,
      ...overrides,
    };
  }

  function provider(): LlmProviderRecord {
    return {
      id: 7,
      kind: "openai-compatible",
      display_name: "默认提供方",
      base_url: "https://example.com/v1",
      api_key: "",
      models: ["glm-4.7", "glm-4.7-air"],
      model_config: {},
      default_model: "glm-4.7",
      is_default: true,
      extra_headers: {},
      secret_status: "ready",
      meta_data: {},
    };
  }

  function composerProps() {
    const activeProvider = provider();
    return {
      modelValue: "",
      templates: [template(), template({ id: 9, display_name: "代码助手" })],
      providers: [activeProvider],
      selectedTemplateId: 3,
      selectedProviderId: 7,
      selectedModel: "glm-4.7",
      disabled: false,
      loading: false,
      placeholder: "请输入内容",
      hasActiveThread: false,
      activeProvider,
      selectedTemplate: template(),
    };
  }

  function mountComposerBar() {
    return mount(ChatComposerBar, {
      props: composerProps(),
    });
  }

  it("keeps the TinyRobot sender in the compact single-line mode", () => {
    const wrapper = mountComposerBar();

    const sender = wrapper.get(".tr-sender-stub");
    expect(sender.attributes("data-mode")).toBe("single");
    expect(sender.attributes("data-size")).toBe("small");
  });

  it("uses the fixed bottom dock chrome for the immersive chat page", () => {
    const wrapper = mountComposerBar();

    expect(wrapper.find(".composer-bar--dock").exists()).toBe(true);
  });

  it("places the message input above a single bottom control row with two chooser buttons", () => {
    const wrapper = mountComposerBar();

    const composer = wrapper.get(".composer-bar");
    const children = Array.from(composer.element.children).map((element) => element.className);
    const footerRow = wrapper.get(".composer-bar__footer-row");

    expect(children[0]).toContain("composer-bar__input-shell");
    expect(children[1]).toContain("composer-bar__footer-row");
    expect(footerRow.findAll("select")).toHaveLength(0);
    expect(footerRow.findAll("input")).toHaveLength(0);
    expect(footerRow.get("[data-testid='agent-picker-trigger']").text()).toContain("通用助手");
    expect(footerRow.get("[data-testid='llm-picker-trigger']").text()).toContain("默认提供方");
    expect(footerRow.get("[data-testid='llm-picker-trigger']").text()).toContain("glm-4.7");
    expect(footerRow.text()).toContain("新对话");
    expect(footerRow.text()).toContain("历史");
    expect(wrapper.find(".composer-bar__control-label").exists()).toBe(false);
  });

  it("uses Tiny icons in the Agent and LLM chooser buttons", () => {
    const wrapper = mountComposerBar();

    expect(wrapper.get("[data-testid='agent-picker-trigger']").find("[data-testid='tiny-icon-user']").exists()).toBe(true);
    expect(wrapper.get("[data-testid='llm-picker-trigger']").find("[data-testid='tiny-icon-ai']").exists()).toBe(true);
  });

  it("provides explicit sender theme variables for background and font sizing", () => {
    const wrapper = mountComposerBar();

    const sender = wrapper.get(".composer-bar__sender").element;
    const styles = window.getComputedStyle(sender);

    expect(styles.getPropertyValue("--tr-sender-bg-color").trim()).not.toBe("");
    expect(styles.getPropertyValue("--tr-sender-font-size-small").trim()).not.toBe("");
    expect(styles.getPropertyValue("--tr-sender-line-height-small").trim()).not.toBe("");
  });

  it("keeps the sender and footer controls inside the dock shell", () => {
    const wrapper = mountComposerBar();

    expect(wrapper.find(".composer-bar__sender").exists()).toBe(true);
    expect(wrapper.find(".composer-bar__footer-row").exists()).toBe(true);
    expect(wrapper.findAll("button")).toHaveLength(5);
  });

  it("emits cancel from the sender while a response is running", async () => {
    const wrapper = mount(ChatComposerBar, {
      props: {
        ...composerProps(),
        loading: true,
      },
    });

    await wrapper.get(".tr-sender-stub button").trigger("click");

    expect(wrapper.emitted("cancel")).toHaveLength(1);
    expect(wrapper.emitted("submit")).toBeUndefined();
  });

  it("opens an agent popover and emits the selected agent", async () => {
    const wrapper = mountComposerBar();

    await wrapper.get("[data-testid='agent-picker-trigger']").trigger("click");

    expect(wrapper.get("[data-testid='agent-picker-popover']").text()).toContain("代码助手");

    await wrapper.get("[data-testid='agent-option-9']").trigger("click");

    expect(wrapper.emitted("update:selectedTemplateId")).toEqual([[9]]);
  });

  it("opens an LLM two-level popover and emits provider and model choices", async () => {
    const wrapper = mountComposerBar();

    await wrapper.get("[data-testid='llm-picker-trigger']").trigger("click");

    expect(wrapper.get("[data-testid='llm-picker-popover']").text()).toContain("默认提供方");
    expect(wrapper.get("[data-testid='model-option-glm-4.7-air']").text()).toBe("glm-4.7-air");

    await wrapper.get("[data-testid='model-option-glm-4.7-air']").trigger("click");

    expect(wrapper.emitted("update:selectedModel")).toEqual([["glm-4.7-air"]]);
  });

  it("shows the typing border on the whole dock instead of only the input area", () => {
    const source = readFileSync("src/features/chat/components/ChatComposerBar.vue", "utf8");

    expect(source).toContain(".composer-bar--dock:focus-within");
    expect(source).toContain("border-color: color-mix(in srgb, var(--accent) 62%, var(--border-default));");
    expect(source).not.toContain(".composer-bar__input-shell:focus-within");
    expect(source).not.toContain(".composer-bar__input-shell {\n  display: flex;\n  align-items: stretch;\n  min-width: 0;\n  padding: 8px 10px;\n  border: 1px solid");
  });
});
