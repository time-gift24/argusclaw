import { defineComponent, h } from "vue";

export default defineComponent({
  name: "OpenTinySwitchStub",
  inheritAttrs: false,
  props: {
    modelValue: {
      type: Boolean,
      default: false,
    },
  },
  emits: ["update:modelValue", "change"],
  setup(props, { attrs, emit }) {
    return () =>
      h("input", {
        ...attrs,
        class: attrs.class,
        type: "checkbox",
        checked: props.modelValue,
        onChange: (event: Event) => {
          const target = event.target as HTMLInputElement;
          emit("update:modelValue", target.checked);
          emit("change", target.checked);
        },
      });
  },
});
