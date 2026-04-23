import { defineComponent, h } from "vue";

export default defineComponent({
  name: "OpenTinyNumericStub",
  inheritAttrs: false,
  props: {
    modelValue: {
      type: [Number, String],
      default: null,
    },
  },
  emits: ["update:modelValue", "change", "input"],
  setup(props, { attrs, emit }) {
    return () =>
      h("input", {
        ...attrs,
        class: attrs.class,
        type: "number",
        value: props.modelValue ?? "",
        onInput: (event: Event) => {
          const target = event.target as HTMLInputElement;
          const nextValue = target.value === "" ? null : Number(target.value);
          emit("update:modelValue", nextValue);
          emit("input", nextValue);
          emit("change", nextValue);
        },
      });
  },
});
