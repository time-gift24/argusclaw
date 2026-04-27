import { mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));
vi.mock("@opentiny/tiny-robot", async () => import("@/test/stubs/tiny-robot"));

import type { LlmProviderRecord } from "@/lib/api";
import ChatComposerBar from "./ChatComposerBar.vue";

describe("ChatComposerBar", () => {
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

  function mountComposerBar() {
    const activeProvider = provider();
    return mount(ChatComposerBar, {
      props: {
        modelValue: "",
        templates: [],
        providers: [activeProvider],
        selectedTemplateId: null,
        selectedProviderId: 7,
        selectedModel: "glm-4.7",
        disabled: false,
        loading: false,
        placeholder: "请输入内容",
        hasActiveThread: false,
        activeProvider,
        selectedTemplate: null,
      },
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

  it("places the message input above a single bottom control row without field titles", () => {
    const wrapper = mountComposerBar();

    const composer = wrapper.get(".composer-bar");
    const children = Array.from(composer.element.children).map((element) => element.className);
    const footerRow = wrapper.get(".composer-bar__footer-row");

    expect(children[0]).toContain("composer-bar__input-shell");
    expect(children[1]).toContain("composer-bar__footer-row");
    expect(footerRow.findAll("select")).toHaveLength(3);
    expect(footerRow.findAll("input")).toHaveLength(0);
    expect(footerRow.text()).toContain("新对话");
    expect(footerRow.text()).toContain("历史");
    expect(wrapper.find(".composer-bar__control-label").exists()).toBe(false);
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
    expect(wrapper.findAll("button")).toHaveLength(3);
  });

  it("renders the active provider models as options and emits the selected model", async () => {
    const wrapper = mountComposerBar();

    const selects = wrapper.findAll("select");
    const modelSelect = selects[2];

    expect(modelSelect.findAll("option")).toHaveLength(2);
    expect(modelSelect.findAll("option")[0]?.text()).toBe("glm-4.7");
    expect(modelSelect.findAll("option")[1]?.text()).toBe("glm-4.7-air");

    await modelSelect.setValue("glm-4.7-air");

    expect(wrapper.emitted("update:selectedModel")).toEqual([["glm-4.7-air"]]);
  });
});
