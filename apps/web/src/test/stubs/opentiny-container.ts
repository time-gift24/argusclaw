import { defineComponent, h } from "vue";

export default defineComponent({
  name: "OpenTinyContainerStub",
  inheritAttrs: false,
  setup(_, { attrs, slots }) {
    return () =>
      h(
        "div",
        {
          ...attrs,
          class: attrs.class,
          "data-opentiny-stub": "container",
        },
        slots.default?.(),
      );
  },
});
