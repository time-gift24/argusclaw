import { computed, defineComponent, h } from "vue";

export default defineComponent({
  name: "OpenTinyGridStub",
  inheritAttrs: false,
  props: {
    data: {
      type: Array,
      default: () => [],
    },
  },
  setup(props, { attrs }) {
    const serialized = computed(() => JSON.stringify(props.data));

    return () =>
      h(
        "div",
        {
          ...attrs,
          class: attrs.class,
          "data-opentiny-stub": "grid",
        },
        serialized.value,
      );
  },
});
