# Web Chat TinyRobot Desktop Polish Design

**Date:** 2026-04-25

**Goal:** Keep the existing `apps/web` TinyRobot + REST/SSE architecture, but upgrade the `/chat` experience to feel closer to the polished desktop chat while adding first-class reasoning and Markdown rendering.

## Scope

- Keep `argus-server` chat REST and thread SSE contracts unchanged.
- Keep `apps/web` independent from desktop chat store and runtime code.
- Continue using TinyRobot primitives for chat UI: `TrBubbleList`, `TrSender`, and `TrPrompts`.
- Add reasoning display based on existing `reasoning_delta` SSE payloads and persisted `reasoning_content`.
- Render assistant text as Markdown instead of plain text.
- Refresh the web chat visual design to align with the desktop chat's polished layout and hierarchy.

## Non-Goals

- No switch to `@opentiny/tiny-robot-kit/useMessage`.
- No desktop store or assistant-ui reuse in `apps/web`.
- No server protocol changes.
- No full tool-call bubble rendering rewrite in this pass.

## Existing Constraints

- Server SSE already emits `content_delta` and `reasoning_delta`.
- Snapshot messages already persist `reasoning_content`.
- Web chat already accumulates pending content and pending reasoning separately.
- The current web chat uses TinyRobot for the stage and sender, but the pending reasoning is only surfaced as placeholder text.
- The current web chat visual shell is functional but flatter and less layered than the desktop chat.

## Chosen Approach

Use the current web chat structure as the system boundary, and improve the presentation layer in place:

1. Extend the TinyRobot message mapping so assistant bubbles can carry both `content` and `reasoning_content`.
2. Configure assistant bubble rendering to use TinyRobot Markdown rendering for visible assistant output.
3. Keep runtime activities outside the bubble list, but visually restyle them to match the desktop chat's artifact language.
4. Restyle the conversation panel, stage, and composer so the page feels like a lighter Vue/TinyRobot adaptation of the desktop chat rather than a plain admin card.

This is the smallest change that upgrades UX without replacing the working data flow.

## Data Flow

### Streaming path

1. `argus-server` emits `chat.thread_event` SSE.
2. `useChatThreadStream.ts` accumulates:
   - `content_delta` into `pendingAssistantContent`
   - `reasoning_delta` into `pendingAssistantReasoning`
3. `useChatPresentation.ts` maps the current settled transcript plus pending assistant state into TinyRobot-ready messages.
4. `ChatMessageStage.vue` renders those messages through `TrBubbleList`.

### Settled path

1. REST snapshot refresh returns persisted assistant messages.
2. If an assistant message contains `reasoning_content`, it should render as a reasoning block in TinyRobot.
3. Assistant `content` should render as Markdown in both settled and streaming states.

## UI Design

### Conversation shell

- Make the chat panel feel like a message workspace instead of a generic admin card.
- Use a softer layered background and stronger visual separation between header, runtime artifacts, message stage, and composer.
- Keep controls compact and Chinese-first, but match the desktop chat's rounded geometry, subtle shadows, and clearer spacing rhythm.

### Message stage

- Increase vertical breathing room around messages.
- Reduce the feeling of a boxed admin panel by using a more atmospheric stage background.
- Keep empty-state prompts, but restyle them to feel more like a lightweight assistant welcome area.

### Assistant messages

- Assistant visible text renders with Markdown.
- Reasoning appears above the visible answer as a collapsible block using TinyRobot's reasoning support.
- Streaming reasoning should visibly transition from "thinking" to settled reasoning content without jumping to a placeholder-only bubble.

### Composer

- Move closer to the desktop pattern: floating bottom surface, rounded shell, stronger focus treatment, and tighter control grouping.
- Keep existing session/template/provider/model controls, but style them as part of the composer system rather than a separate settings bar.

### Runtime activity

- Keep the current separate panel for retries, tool activity, and notices.
- Restyle it to feel like desktop artifacts: compact cards, status-led visuals, stronger grouping, and better emphasis on running/error states.

## Error Handling

- If SSE disconnects, keep the current refresh fallback behavior.
- If Markdown renderer setup fails, the message should still fall back to plain content rather than breaking the chat page.
- If reasoning is present without visible content, still render the reasoning block and a lightweight assistant pending/empty state.

## Testing Strategy

- Extend presentation tests to verify assistant message mapping now preserves `reasoning_content`.
- Extend stream tests to verify reasoning deltas survive into the pending assistant message model.
- Extend page/component tests to verify the stage uses the intended rendering hooks for reasoning and Markdown.
- Run targeted Vitest chat tests first, then run `pnpm build` for `apps/web`.

## Risks

- TinyRobot renderer wiring may require Bubble-level configuration rather than just message shape changes.
- Styling changes can regress layout density on smaller screens if desktop spacing is copied too literally.
- If the TinyRobot list-level API differs from the single-bubble Markdown demo path, the renderer may need a provider wrapper or role-based fallback renderer configuration.

## Acceptance Criteria

- Web chat remains on the existing REST/SSE architecture.
- Streaming assistant output renders in a single pending assistant bubble.
- `reasoning_delta` and persisted `reasoning_content` both appear as visible reasoning UI.
- Assistant answers render as Markdown.
- The `/chat` page looks materially closer to the polished desktop chat than the current admin-card version.
