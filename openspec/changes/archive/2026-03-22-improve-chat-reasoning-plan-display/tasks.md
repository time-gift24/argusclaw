## 1. Reasoning Block Rearrangement

- [x] 1.1 Reverse the order of `reasoning` and `text` parts in `buildAssistantUiMessages` in `chat-runtime.ts` so reasoning appears after text in the content array
- [x] 1.2 Update `ReasoningBlock` component in `thread.tsx` to remove the collapsible summary wrapper and always render expanded

## 2. Reasoning Block Fixed Height & Auto-Scroll

- [x] 2.1 Apply fixed max-height to `ReasoningBlock` container (approximately 150-200px) using Tailwind (`max-h-[150px] overflow-y-auto`)
- [x] 2.2 Add auto-scroll behavior using a `useRef` and `useEffect` to scroll to `scrollHeight` when reasoning content updates during streaming

## 3. Plan Panel Early Display (tool_started)

- [x] 3.1 Add `tool_started` event handler in `chat-store.ts` that detects `update_plan` and initializes `pendingAssistant.plan` from tool arguments
- [x] 3.2 Ensure `tool_completed` event handler for `update_plan` still updates plan to final state (overwrites initial from `tool_started`)

## 4. Plan Panel In-Message Layout

- [x] 4.1 Move `PlanPanel` from thread viewport level to inside `AssistantMessage` component, positioned below `ReasoningBlock` and above `MessageError` / action bar
- [x] 4.2 Update `PlanPanel` width styling to match message content width (`w-full` instead of `max-w-*`)
- [x] 4.3 Conditionally render PlanPanel only when `session.pendingAssistant.plan` is non-null

## 5. Remove Legacy Plan Panel

- [x] 5.1 Remove the `PlanPanel` usage from `ThreadViewport` in `thread.tsx` (the top-level `{session?.plan && <PlanPanel plan={session.plan} />}` line)
- [x] 5.2 Verify PlanPanel is only rendered inside AssistantMessage going forward
