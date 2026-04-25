import { computed, defineComponent, h } from "vue";

export default defineComponent({
  name: "OpenTinyInputStub",
  inheritAttrs: false,
  props: {
    modelValue: {
      type: [String, Number],
      default: "",
    },
  },
  emits: ["update:modelValue", "change", "input"],
  setup(props, { attrs, emit }) {
    const isTextarea = computed(() => attrs.type === "textarea");
    const type = computed(() => (typeof props.modelValue === "number" ? "number" : "text"));
    const handleInput = (event: Event) => {
      const target = event.target as HTMLInputElement | HTMLTextAreaElement;
      const nextValue = type.value === "number" ? Number(target.value) : target.value;
      emit("update:modelValue", nextValue);
      emit("input", nextValue);
      emit("change", nextValue);
    };

    return () => {
      if (isTextarea.value) {
        return h("textarea", {
          ...attrs,
          class: attrs.class,
          value: props.modelValue,
          onInput: handleInput,
        });
      }

      return h("input", {
        ...attrs,
        class: attrs.class,
        type: type.value,
        value: props.modelValue,
        onInput: handleInput,
      });
    };
  },
});
