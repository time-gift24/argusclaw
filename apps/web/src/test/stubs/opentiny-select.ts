import { defineComponent, h } from "vue";

export default defineComponent({
  name: "OpenTinySelectStub",
  inheritAttrs: false,
  props: {
    modelValue: {
      type: [String, Number, Array],
      default: "",
    },
  },
  emits: ["update:modelValue", "change"],
  setup(props, { attrs, emit, slots }) {
    return () =>
      h(
        "select",
        {
          ...attrs,
          class: attrs.class,
          value: props.modelValue,
          onChange: (event: Event) => {
            const target = event.target as HTMLSelectElement;
            emit("update:modelValue", target.value);
            emit("change", target.value);
          },
        },
        slots.default?.(),
      );
  },
});
