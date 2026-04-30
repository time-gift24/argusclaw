import { defineComponent, h, inject, provide, type PropType } from "vue";

interface PromptItem {
  id?: string;
  label: string;
  description?: string;
}

interface BubbleMessage {
  id?: string;
  role?: string;
  content?: unknown;
  reasoning_content?: string;
}

interface BubbleContentRendererMatch {
  find: (message: BubbleMessage, content: Record<string, unknown>, contentIndex: number) => boolean;
  renderer: unknown;
}

const BUBBLE_MATCHES_KEY = Symbol("bubble-content-renderer-matches");
const BUBBLE_FALLBACK_KEY = Symbol("bubble-fallback-content-renderer");

export const TrBubbleList = defineComponent({
  name: "TrBubbleList",
  props: {
    messages: {
      type: Array as PropType<BubbleMessage[]>,
      default: () => [],
    },
    contentResolver: {
      type: Function as PropType<(message: BubbleMessage) => unknown>,
      default: undefined,
    },
  },
  setup(props) {
    const contentRendererMatches = inject<BubbleContentRendererMatch[]>(BUBBLE_MATCHES_KEY, []);
    inject<unknown>(BUBBLE_FALLBACK_KEY, null);

    function renderContent(message: BubbleMessage) {
      const resolved = props.contentResolver ? props.contentResolver(message) : message.content;

      if (Array.isArray(resolved)) {
        return resolved.map((content, contentIndex) => {
          const normalized = typeof content === "string"
            ? { type: "text", text: content }
            : content;
          const match = contentRendererMatches.find((entry) =>
            entry.find(message, normalized as Record<string, unknown>, contentIndex));

          if (match) {
            return h(match.renderer as never, {
              key: `content-${contentIndex}`,
              message: {
                ...message,
                content: resolved,
              },
              contentIndex,
            });
          }

          if (
            normalized
            && typeof normalized === "object"
            && "type" in normalized
            && (normalized as { type?: string }).type === "text"
          ) {
            return h("div", { key: `content-${contentIndex}` }, (normalized as { text?: string }).text ?? "");
          }

          return h("div", { key: `content-${contentIndex}` }, JSON.stringify(normalized));
        });
      }

      return typeof resolved === "string" ? resolved : "";
    }

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
              "data-reasoning": message.reasoning_content ?? "",
              key: `${message.role ?? "message"}-${index}`,
            },
            renderContent(message),
          ),
        ),
      );
  },
});

export const TrBubbleProvider = defineComponent({
  name: "TrBubbleProvider",
  props: {
    contentRendererMatches: {
      type: Array as PropType<BubbleContentRendererMatch[]>,
      default: () => [],
    },
    fallbackContentRenderer: {
      type: [Object, Function, String] as PropType<unknown>,
      default: null,
    },
  },
  setup(props, { slots }) {
    provide(BUBBLE_MATCHES_KEY, props.contentRendererMatches);
    provide(BUBBLE_FALLBACK_KEY, props.fallbackContentRenderer);
    return () =>
      h(
        "div",
        {
          class: "tr-bubble-provider-stub",
          "data-fallback-content-renderer":
            props.fallbackContentRenderer == null ? "unset" : "set",
        },
        slots.default?.(),
      );
  },
});

export const BubbleRenderers = {
  Markdown: "markdown-renderer-stub",
};

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
    mode: {
      type: String,
      default: "multiple",
    },
    size: {
      type: String,
      default: "normal",
    },
  },
  emits: ["update:modelValue", "submit", "cancel"],
  setup(props, { emit }) {
    return () =>
      h("div", { class: "tr-sender-stub", "data-mode": props.mode, "data-size": props.size }, [
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
