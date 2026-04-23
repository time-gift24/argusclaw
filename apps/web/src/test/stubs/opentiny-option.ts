import { defineComponent, h } from "vue";

export default defineComponent({
  name: "OpenTinyOptionStub",
  inheritAttrs: false,
  props: {
    label: {
      type: String,
      default: "",
    },
    value: {
      type: [String, Number],
      default: "",
    },
  },
  setup(props, { slots }) {
    return () => h("option", { value: props.value }, slots.default?.() ?? props.label);
  },
});
