import { defineComponent, h } from "vue";

export default defineComponent({
  name: "OpenTinyGridColumnStub",
  setup() {
    return () => h("div", { hidden: true });
  },
});
