import { defineComponent, h, type PropType } from "vue";

interface PromptItem {
  id?: string;
  label: string;
  description?: string;
}

interface BubbleMessage {
  role?: string;
  content?: string;
}

export const TrBubbleList = defineComponent({
  name: "TrBubbleList",
  props: {
    messages: {
      type: Array as PropType<BubbleMessage[]>,
      default: () => [],
    },
  },
  setup(props) {
    return () =>
      h(
        "div",
        { class: "tr-bubble-list-stub" },
        props.messages.map((message, index) =>
          h(
            "article",
            {
              class: "tr-bubble-stub",
              "data-role": message.role,
              key: `${message.role ?? "message"}-${index}`,
            },
            message.content,
          ),
        ),
      );
  },
});

export const TrSender = defineComponent({
  name: "TrSender",
  props: {
    modelValue: {
      type: String,
      default: "",
    },
    placeholder: {
      type: String,
      default: "",
    },
    disabled: {
      type: Boolean,
      default: false,
    },
    loading: {
      type: Boolean,
      default: false,
    },
  },
  emits: ["update:modelValue", "submit", "cancel"],
  setup(props, { emit }) {
    return () =>
      h("div", { class: "tr-sender-stub" }, [
        h("input", {
          "data-testid": "chat-input",
          disabled: props.disabled,
          placeholder: props.placeholder,
          value: props.modelValue,
          onInput: (event: Event) => emit("update:modelValue", (event.target as HTMLInputElement).value),
          onKeydown: (event: KeyboardEvent) => {
            if (event.key === "Enter") {
              emit("submit", (event.target as HTMLInputElement).value);
            }
          },
        }),
        h(
          "button",
          {
            disabled: props.disabled,
            type: "button",
            onClick: () => emit("submit", props.modelValue),
          },
          props.loading ? "发送中" : "发送",
        ),
      ]);
  },
});

export const TrPrompts = defineComponent({
  name: "TrPrompts",
  props: {
    items: {
      type: Array as PropType<PromptItem[]>,
      default: () => [],
    },
  },
  emits: ["item-click"],
  setup(props, { emit }) {
    return () =>
      h(
        "div",
        { class: "tr-prompts-stub" },
        props.items.map((item) =>
          h(
            "button",
            {
              "data-testid": item.id ? `prompt-${item.id}` : undefined,
              key: item.id ?? item.label,
              type: "button",
              onClick: (event: MouseEvent) => emit("item-click", event, item),
            },
            [h("strong", item.label), item.description ? h("span", item.description) : null],
          ),
        ),
      );
  },
});
