import { mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/lib/opentiny", async () => import("@/test/stubs/opentiny"));
vi.mock("@opentiny/tiny-robot", async () => import("@/test/stubs/tiny-robot"));

import ChatComposerBar from "./ChatComposerBar.vue";

describe("ChatComposerBar", () => {
  it("keeps the TinyRobot sender in the compact single-line mode", () => {
    const wrapper = mount(ChatComposerBar, {
      props: {
        modelValue: "",
        templates: [],
        providers: [],
        selectedTemplateId: null,
        selectedProviderId: null,
        selectedModel: "",
        disabled: false,
        loading: false,
        placeholder: "请输入内容",
        hasActiveThread: false,
        activeProvider: null,
        selectedTemplate: null,
      },
    });

    const sender = wrapper.get(".tr-sender-stub");
    expect(sender.attributes("data-mode")).toBe("single");
    expect(sender.attributes("data-size")).toBe("small");
  });

  it("uses the fixed bottom dock chrome for the immersive chat page", () => {
    const wrapper = mount(ChatComposerBar, {
      props: {
        modelValue: "",
        templates: [],
        providers: [],
        selectedTemplateId: null,
        selectedProviderId: null,
        selectedModel: "",
        disabled: false,
        loading: false,
        placeholder: "请输入内容",
        hasActiveThread: false,
        activeProvider: null,
        selectedTemplate: null,
      },
    });

    expect(wrapper.find(".composer-bar--dock").exists()).toBe(true);
  });

  it("places the message input above a single bottom control row without field titles", () => {
    const wrapper = mount(ChatComposerBar, {
      props: {
        modelValue: "",
        templates: [],
        providers: [],
        selectedTemplateId: null,
        selectedProviderId: null,
        selectedModel: "",
        disabled: false,
        loading: false,
        placeholder: "请输入内容",
        hasActiveThread: false,
        activeProvider: null,
        selectedTemplate: null,
      },
    });

    const composer = wrapper.get(".composer-bar");
    const children = Array.from(composer.element.children).map((element) => element.className);
    const footerRow = wrapper.get(".composer-bar__footer-row");

    expect(children[0]).toContain("composer-bar__input-shell");
    expect(children[1]).toContain("composer-bar__footer-row");
    expect(footerRow.classes()).toContain("composer-bar__footer-row--compact");
    expect(footerRow.findAll("select")).toHaveLength(2);
    expect(footerRow.findAll("input")).toHaveLength(1);
    expect(footerRow.text()).toContain("新对话");
    expect(footerRow.text()).toContain("历史");
    expect(wrapper.find(".composer-bar__control-label").exists()).toBe(false);
  });

  it("provides explicit sender theme variables for background and font sizing", () => {
    const wrapper = mount(ChatComposerBar, {
      props: {
        modelValue: "",
        templates: [],
        providers: [],
        selectedTemplateId: null,
        selectedProviderId: null,
        selectedModel: "",
        disabled: false,
        loading: false,
        placeholder: "请输入内容",
        hasActiveThread: false,
        activeProvider: null,
        selectedTemplate: null,
      },
      attachTo: document.body,
    });

    const sender = wrapper.get(".composer-bar__sender").element;
    const styles = window.getComputedStyle(sender);

    expect(styles.getPropertyValue("--tr-sender-bg-color").trim()).not.toBe("");
    expect(styles.getPropertyValue("--tr-sender-font-size-small").trim()).not.toBe("");
    expect(styles.getPropertyValue("--tr-sender-line-height-small").trim()).not.toBe("");
  });

  it("uses borderless chrome for the sender and bottom controls", () => {
    const wrapper = mount(ChatComposerBar, {
      props: {
        modelValue: "",
        templates: [],
        providers: [],
        selectedTemplateId: null,
        selectedProviderId: null,
        selectedModel: "",
        disabled: false,
        loading: false,
        placeholder: "请输入内容",
        hasActiveThread: false,
        activeProvider: null,
        selectedTemplate: null,
      },
    });

    expect(wrapper.find(".composer-bar__sender--borderless").exists()).toBe(true);
    expect(wrapper.findAll(".composer-bar__plain-control")).toHaveLength(5);
  });
});
