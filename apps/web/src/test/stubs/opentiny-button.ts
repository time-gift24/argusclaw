import { defineComponent, h } from "vue";

export default defineComponent({
  name: "OpenTinyButtonStub",
  inheritAttrs: false,
  emits: ["click"],
  setup(_, { attrs, emit, slots }) {
    return () =>
      h(
        "button",
        {
          ...attrs,
          type: (attrs.type as string) || "button",
          class: attrs.class,
          onClick: (event: MouseEvent) => emit("click", event),
        },
        slots.default?.(),
      );
  },
});
