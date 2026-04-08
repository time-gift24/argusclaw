# Subagent Job Details Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a right-side details drawer for subagent jobs in desktop chat so users can inspect final output and execution progress instead of only seeing summary cards.

**Architecture:** Keep the existing `jobStatuses` store slice for lightweight card rendering and add a separate `jobDetails` slice for drawer data. Feed `jobDetails` from `job_dispatched`, `thread_pool_*`, `job_result`, and `mailbox_message_queued`, then render a drawer component driven by the selected job id in the active session.

**Tech Stack:** React 19, TypeScript, Zustand, Base UI dialog primitives, Node built-in test runner, existing desktop regex/smoke tests

---

### Task 1: Extend chat event and store types for job details

**Files:**
- Modify: `crates/desktop/lib/types/chat.ts`
- Modify: `crates/desktop/lib/chat-store.ts`
- Test: `crates/desktop/tests/chat-store-session-model.test.mjs`

**Step 1: Write the failing test**

Add assertions to `crates/desktop/tests/chat-store-session-model.test.mjs` for:

- `ChatSessionState` including `jobDetails`
- `ThreadEventPayload` including `mailbox_message_queued`
- a dedicated `JobDetailPayload`-style type or equivalent detail shape

**Step 2: Run test to verify it fails**

Run: `pnpm test -- chat-store-session-model.test.mjs`

Expected: FAIL because the store and type definitions do not yet mention `jobDetails` or mailbox-backed detail handling.

**Step 3: Write minimal implementation**

In `crates/desktop/lib/types/chat.ts`:

- add a typed shape for job detail state
- add a typed shape for mailbox payload if needed by the store

In `crates/desktop/lib/chat-store.ts`:

- extend `ChatSessionState` with `jobDetails`
- add any helper types needed for a per-job timeline

**Step 4: Run test to verify it passes**

Run: `pnpm test -- chat-store-session-model.test.mjs`

Expected: PASS for the new structural assertions.

**Step 5: Commit**

```bash
git add crates/desktop/lib/types/chat.ts crates/desktop/lib/chat-store.ts crates/desktop/tests/chat-store-session-model.test.mjs
git commit -m "test: define desktop subagent job detail state"
```

### Task 2: Build store helpers for summary/detail normalization

**Files:**
- Modify: `crates/desktop/lib/chat-store.ts`
- Create: `crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

**Step 1: Write the failing test**

Create `crates/desktop/tests/chat-store-subagent-job-details.test.mjs` with assertions that the store source now contains:

- a helper to normalize detail payloads
- a helper to append timeline events
- a helper or branch that derives full result text from mailbox job results

**Step 2: Run test to verify it fails**

Run: `node --test crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

Expected: FAIL because none of the new helper names or branches exist yet.

**Step 3: Write minimal implementation**

In `crates/desktop/lib/chat-store.ts`:

- add a `normalizeJobDetailPayload` helper
- add a timeline append/update helper
- keep card truncation rules for `jobStatuses`
- keep drawer-facing `result_text` less aggressively truncated than summary fields

**Step 4: Run test to verify it passes**

Run: `node --test crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/desktop/lib/chat-store.ts crates/desktop/tests/chat-store-subagent-job-details.test.mjs
git commit -m "test: add subagent job detail store helpers"
```

### Task 3: Feed job details from `job_dispatched` and `job_result`

**Files:**
- Modify: `crates/desktop/lib/chat-store.ts`
- Modify: `crates/desktop/tests/chat-store-session-model.test.mjs`
- Modify: `crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

**Step 1: Write the failing test**

Add assertions for:

- `case "job_dispatched"` initializing `jobDetails[payload.job_id]`
- `case "job_result"` updating status, summary text, tokens, and completion timestamp in detail state

**Step 2: Run test to verify it fails**

Run: `pnpm test -- chat-store-session-model.test.mjs`

Expected: FAIL because the existing branches only update `jobStatuses`.

**Step 3: Write minimal implementation**

Update both branches in `crates/desktop/lib/chat-store.ts` to:

- initialize a detail record on dispatch
- preserve prompt and agent metadata
- write summary/result metadata on completion without touching transcript messages

**Step 4: Run test to verify it passes**

Run: `pnpm test -- chat-store-session-model.test.mjs`

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/desktop/lib/chat-store.ts crates/desktop/tests/chat-store-session-model.test.mjs crates/desktop/tests/chat-store-subagent-job-details.test.mjs
git commit -m "feat: persist subagent job detail summaries in store"
```

### Task 4: Feed job details from mailbox and thread-pool events

**Files:**
- Modify: `crates/desktop/lib/chat-store.ts`
- Modify: `crates/desktop/lib/types/chat.ts`
- Modify: `crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

**Step 1: Write the failing test**

Add assertions for:

- `case "mailbox_message_queued"` existing in the store
- filtering mailbox payloads to `job_result` messages
- using mailbox `text` as the preferred full result body
- `thread_pool_queued|started|cooling|evicted` updating a per-job timeline when `runtime.job_id` exists

**Step 2: Run test to verify it fails**

Run: `node --test crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

Expected: FAIL because mailbox events are not currently consumed.

**Step 3: Write minimal implementation**

In `crates/desktop/lib/chat-store.ts`:

- add a `mailbox_message_queued` switch branch
- guard against unrelated mailbox message types
- prefer mailbox `text` over summary text for drawer display
- extend thread-pool branches to append timeline entries keyed by `runtime.job_id`

**Step 4: Run test to verify it passes**

Run: `node --test crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/desktop/lib/chat-store.ts crates/desktop/lib/types/chat.ts crates/desktop/tests/chat-store-subagent-job-details.test.mjs
git commit -m "feat: hydrate subagent job details from mailbox events"
```

### Task 5: Add drawer selection state to the active chat session

**Files:**
- Modify: `crates/desktop/lib/chat-store.ts`
- Modify: `crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

**Step 1: Write the failing test**

Add assertions for:

- selected job detail id state in the store or session model
- actions/selectors to open and close the job details drawer

**Step 2: Run test to verify it fails**

Run: `node --test crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

Expected: FAIL because there is no selection state for a drawer.

**Step 3: Write minimal implementation**

In `crates/desktop/lib/chat-store.ts`:

- add selected job detail state at the session or store level
- add `openJobDetails(jobId)` and `closeJobDetails()` actions
- ensure switching sessions/threads clears stale selection safely

**Step 4: Run test to verify it passes**

Run: `node --test crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/desktop/lib/chat-store.ts crates/desktop/tests/chat-store-subagent-job-details.test.mjs
git commit -m "feat: track selected subagent job details"
```

### Task 6: Create the right-side job details drawer component

**Files:**
- Create: `crates/desktop/components/assistant-ui/subagent-job-details-drawer.tsx`
- Modify: `crates/desktop/components/ui/dialog.tsx`
- Test: `crates/desktop/tests/chat-subagent-job-details-drawer.test.mjs`

**Step 1: Write the failing test**

Create `crates/desktop/tests/chat-subagent-job-details-drawer.test.mjs` with source assertions for:

- a dedicated drawer component file
- use of the existing dialog primitives for a right-side drawer layout
- sections for final output, task info, and execution timeline
- fallback copy for missing full result text

**Step 2: Run test to verify it fails**

Run: `node --test crates/desktop/tests/chat-subagent-job-details-drawer.test.mjs`

Expected: FAIL because the component file does not exist.

**Step 3: Write minimal implementation**

Create `crates/desktop/components/assistant-ui/subagent-job-details-drawer.tsx` that:

- reads one selected job detail payload
- renders a right-side drawer via `Dialog` primitives
- shows final output first
- shows summary fallback when full output is missing
- shows prompt, status, job id, token usage, and timeline

Update `crates/desktop/components/ui/dialog.tsx` only if needed to support a reusable side-drawer class pattern without regressing existing dialogs.

**Step 4: Run test to verify it passes**

Run: `node --test crates/desktop/tests/chat-subagent-job-details-drawer.test.mjs`

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/desktop/components/assistant-ui/subagent-job-details-drawer.tsx crates/desktop/components/ui/dialog.tsx crates/desktop/tests/chat-subagent-job-details-drawer.test.mjs
git commit -m "feat: add desktop subagent job details drawer"
```

### Task 7: Wire task cards to open the drawer

**Files:**
- Modify: `crates/desktop/components/assistant-ui/thread.tsx`
- Modify: `crates/desktop/tests/chat-page-runtime-integration.test.mjs`
- Modify: `crates/desktop/tests/chat-thread-scroll-layout.test.mjs`

**Step 1: Write the failing test**

Add assertions that `thread.tsx`:

- renders the new details drawer component
- makes each job card actionable
- opens details from the job card without changing transcript rendering

**Step 2: Run test to verify it fails**

Run: `pnpm test -- chat-page-runtime-integration.test.mjs`

Expected: FAIL because the thread UI does not yet render or open the drawer.

**Step 3: Write minimal implementation**

In `crates/desktop/components/assistant-ui/thread.tsx`:

- import the new drawer component
- make job cards clickable or add a clear “查看详情” action
- wire click handlers to `openJobDetails(job.job_id)`
- mount one drawer component per active chat session

**Step 4: Run test to verify it passes**

Run: `pnpm test -- chat-page-runtime-integration.test.mjs`

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/desktop/components/assistant-ui/thread.tsx crates/desktop/tests/chat-page-runtime-integration.test.mjs crates/desktop/tests/chat-thread-scroll-layout.test.mjs
git commit -m "feat: open subagent job details from chat cards"
```

### Task 8: Add fallback and failure-state coverage

**Files:**
- Modify: `crates/desktop/components/assistant-ui/subagent-job-details-drawer.tsx`
- Modify: `crates/desktop/tests/chat-subagent-job-details-drawer.test.mjs`
- Modify: `crates/desktop/tests/chat-store-subagent-job-details.test.mjs`

**Step 1: Write the failing test**

Add assertions for:

- failed jobs rendering failure styling and failure copy
- completed jobs without mailbox body rendering summary fallback
- ignored mailbox events not crashing the selection flow

**Step 2: Run test to verify it fails**

Run: `pnpm test -- chat-store-subagent-job-details.test.mjs`

Expected: FAIL because fallback/error coverage is incomplete.

**Step 3: Write minimal implementation**

Implement:

- explicit fallback label such as “结果摘要”
- failure-state display for the final output block
- no-op handling for unmatched mailbox messages

**Step 4: Run test to verify it passes**

Run: `pnpm test -- chat-store-subagent-job-details.test.mjs`

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/desktop/components/assistant-ui/subagent-job-details-drawer.tsx crates/desktop/tests/chat-subagent-job-details-drawer.test.mjs crates/desktop/tests/chat-store-subagent-job-details.test.mjs
git commit -m "test: cover subagent job detail fallback states"
```

### Task 9: Run desktop verification and clean up

**Files:**
- Modify: `crates/desktop/lib/chat-store.ts`
- Modify: `crates/desktop/components/assistant-ui/thread.tsx`
- Modify: `crates/desktop/components/assistant-ui/subagent-job-details-drawer.tsx`

**Step 1: Run focused tests**

Run:

```bash
pnpm test -- chat-store-session-model.test.mjs
node --test crates/desktop/tests/chat-store-subagent-job-details.test.mjs
node --test crates/desktop/tests/chat-subagent-job-details-drawer.test.mjs
pnpm test -- chat-page-runtime-integration.test.mjs
```

Expected: PASS for all targeted desktop regressions.

**Step 2: Run broader desktop suite**

Run:

```bash
pnpm test
```

Expected: PASS with no regressions in existing desktop source-shape tests.

**Step 3: Run lint if UI code changed materially**

Run:

```bash
pnpm lint
```

Expected: PASS or only pre-existing unrelated warnings.

**Step 4: Make minimal cleanup edits**

Remove dead helpers, tighten naming, and keep the drawer text concise.

**Step 5: Commit**

```bash
git add crates/desktop/lib/chat-store.ts crates/desktop/components/assistant-ui/thread.tsx crates/desktop/components/assistant-ui/subagent-job-details-drawer.tsx crates/desktop/tests/chat-store-session-model.test.mjs crates/desktop/tests/chat-store-subagent-job-details.test.mjs crates/desktop/tests/chat-subagent-job-details-drawer.test.mjs crates/desktop/tests/chat-page-runtime-integration.test.mjs crates/desktop/tests/chat-thread-scroll-layout.test.mjs
git commit -m "feat: surface subagent job details in desktop chat"
```
