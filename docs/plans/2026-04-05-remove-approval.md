# Remove Current Approval Logic Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove the current approval system end-to-end so chat and tool execution run without approval-specific protocol, runtime state, Tauri bindings, or frontend UI.

**Architecture:** Treat approval as a removable vertical slice, not as a cross-cutting concern to preserve. Delete the dedicated crate and public protocol/API surfaces first, then collapse agent runtime branches and desktop state/UI back to the normal execution path. Keep `RiskLevel` intact as tool metadata.

**Tech Stack:** Rust workspace (`cargo`), Tauri 2, React 19, TypeScript, Node test runner, `tsx`

---

### Task 1: Create documentation artifacts and pin the deletion boundary

**Files:**
- Create: `docs/plans/2026-04-05-remove-approval-design.md`
- Create: `docs/plans/2026-04-05-remove-approval.md`

**Step 1: Write the design doc**

Document:

- why the current approval slice should be fully removed
- why `RiskLevel` stays
- which crates and files are in scope
- what verification proves completion

**Step 2: Write the implementation plan**

Include exact files, commands, and the order of deletion so a fresh engineer can execute safely.

**Step 3: Commit the docs**

Run:

```bash
git add docs/plans/2026-04-05-remove-approval-design.md docs/plans/2026-04-05-remove-approval.md
git commit -m "Record the approval removal plan

Constraint: Current approval slice is partially wired and blocks redesign clarity
Rejected: Preserve compatibility seam | would keep stale approval abstractions alive
Confidence: high
Scope-risk: narrow
Directive: Keep RiskLevel as tool metadata even after approval removal
Tested: Design and implementation boundaries reviewed against current code layout
Not-tested: Runtime behavior changes not yet executed"
```

### Task 2: Remove the dedicated approval crate and core wing API

**Files:**
- Modify: `Cargo.toml`
- Delete: `crates/argus-approval/Cargo.toml`
- Delete: `crates/argus-approval/CLAUDE.md`
- Delete: `crates/argus-approval/src/lib.rs`
- Delete: `crates/argus-approval/src/error.rs`
- Delete: `crates/argus-approval/src/hook.rs`
- Delete: `crates/argus-approval/src/manager.rs`
- Delete: `crates/argus-approval/src/policy.rs`
- Delete: `crates/argus-approval/src/runtime_allow.rs`
- Modify: `crates/argus-wing/Cargo.toml`
- Modify: `crates/argus-wing/src/lib.rs`

**Step 1: Write the failing compile change**

Remove the workspace member and `argus-wing` dependency on `argus-approval`, then remove the corresponding imports and fields in `argus-wing/src/lib.rs`.

Expected failure:

- compiler errors where `approval_manager`, `ApprovalPolicy`, or approval API methods are still referenced

**Step 2: Delete the public approval API from `ArgusWing`**

Remove:

- `approval_manager` field and constructor wiring
- `approval_manager()` accessor
- `create_session_with_approval(...)`
- `list_pending_approvals(...)`
- `resolve_approval(...)`
- approval-focused tests

**Step 3: Run targeted Rust tests**

Run:

```bash
cargo test -p argus-wing
```

Expected:

- `argus-wing` compiles without `argus-approval`
- remaining failures point only to protocol/desktop call sites not yet updated

**Step 4: Commit the crate/API removal**

```bash
git add Cargo.toml crates/argus-wing/Cargo.toml crates/argus-wing/src/lib.rs crates/argus-approval
git commit -m "Remove the obsolete approval crate from the runtime surface

Constraint: Approval is not part of the active thread construction path
Rejected: Leave wing approval APIs as stubs | stale APIs would mislead future work
Confidence: medium
Scope-risk: moderate
Directive: Reintroduce human-in-the-loop only from a fresh protocol design
Tested: cargo test -p argus-wing
Not-tested: Desktop integration not updated yet"
```

### Task 3: Delete approval protocol types and thread events while preserving `RiskLevel`

**Files:**
- Modify: `crates/argus-protocol/src/lib.rs`
- Modify: `crates/argus-protocol/src/events.rs`
- Delete: `crates/argus-protocol/src/approval.rs`

**Step 1: Write the failing compile change**

Remove:

- `pub mod approval;`
- `pub use approval::{...};`
- `ThreadEvent::WaitingForApproval`
- `ThreadEvent::ApprovalResolved`

Do not change:

- `crates/argus-protocol/src/risk_level.rs`
- any `NamedTool::risk_level()` contract

**Step 2: Run targeted protocol tests**

Run:

```bash
cargo test -p argus-protocol
```

Expected:

- protocol compiles without approval types
- downstream compile failures identify agent/desktop references to remove next

**Step 3: Commit protocol cleanup**

```bash
git add crates/argus-protocol/src/lib.rs crates/argus-protocol/src/events.rs crates/argus-protocol/src/approval.rs
git commit -m "Remove approval-specific protocol types and events

Constraint: Tool risk metadata remains useful without approval flow
Rejected: Rename approval events into generic interrupts | no consumer needs that seam today
Confidence: high
Scope-risk: moderate
Directive: Keep RiskLevel independent from future approval or policy systems
Tested: cargo test -p argus-protocol
Not-tested: Agent and desktop downstream references before cleanup"
```

### Task 4: Collapse `argus-agent` back to a single execution path

**Files:**
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/tests/integration_test.rs`

**Step 1: Remove approval-specific progress and runtime state**

Delete:

- `TurnProgress::WaitingForApproval`
- `TurnProgress::ApprovalResolved`
- approval event mapping/draining helpers
- `ThreadRuntimeState::WaitingForApproval`
- runtime transitions that pause/resume around approval

**Step 2: Remove approval-only test helpers**

Delete blocking approval hook fixtures and any tests asserting approval wait/resume behavior.

Keep:

- unrelated hook infrastructure
- normal turn progress behavior

**Step 3: Run targeted agent tests**

Run:

```bash
cargo test -p argus-agent
```

Expected:

- no approval terms remain in runtime state or progress
- remaining failures, if any, are from desktop or docs references

**Step 4: Commit agent runtime cleanup**

```bash
git add crates/argus-agent/src/turn.rs crates/argus-agent/src/thread.rs crates/argus-agent/tests/integration_test.rs
git commit -m "Collapse agent runtime after removing approval pauses

Constraint: Thread execution should only reflect active runtime behavior
Rejected: Preserve waiting state as a generic pause flag | unnecessary without a live producer
Confidence: medium
Scope-risk: moderate
Directive: Add new human-interrupt states only with an end-to-end consumer path
Tested: cargo test -p argus-agent
Not-tested: Desktop rendering path before frontend cleanup"
```

### Task 5: Remove desktop/Tauri approval state, UI, and bindings

**Files:**
- Delete: `crates/desktop/components/chat/approval-prompt.tsx`
- Modify: `crates/desktop/components/assistant-ui/thread.tsx`
- Modify: `crates/desktop/lib/chat-store.ts`
- Modify: `crates/desktop/lib/chat-runtime.ts`
- Modify: `crates/desktop/lib/types/chat.ts`
- Modify: `crates/desktop/lib/tauri.ts`
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Modify: `crates/desktop/src-tauri/src/events/thread.rs`
- Modify: `crates/desktop/src-tauri/src/lib.rs`
- Modify: `crates/desktop/tests/chat-selector-flow.test.mjs`
- Modify: `crates/desktop/tests/chat-store-session-model.test.mjs`
- Modify: `crates/desktop/tests/chat-tauri-bindings.test.mjs`

**Step 1: Remove approval UI mount and component**

Delete `approval-prompt.tsx` and remove its import/render from `thread.tsx`.

**Step 2: Remove approval state from chat store and runtime**

Delete:

- `pendingApprovalRequest`
- `waiting_for_approval` / `approval_resolved` event handling
- pending assistant `requires-action` state

Replace with the normal running assistant status.

**Step 3: Remove Tauri command and event wiring**

Delete:

- `chat.resolveApproval(...)`
- desktop `ApprovalDecision` type
- `resolve_approval` Rust command
- Tauri registration entry
- approval event payload variants

**Step 4: Rewrite tests around the new baseline**

Make tests assert that:

- thread composer no longer mounts approval UI
- store session model no longer expects approval events
- Tauri bindings no longer expose approval resolution

**Step 5: Run desktop verification**

Run:

```bash
cd crates/desktop
pnpm test
pnpm build
```

Expected:

- node/tsx tests pass
- TypeScript/Vite build passes without approval-specific types

**Step 6: Commit desktop cleanup**

```bash
git add crates/desktop
git commit -m "Remove approval UI and Tauri bindings from desktop chat

Constraint: Desktop must not depend on backend events that no longer exist
Rejected: Hide the UI but keep bindings | hidden dead code would preserve stale protocol contracts
Confidence: medium
Scope-risk: moderate
Directive: Future confirmation UX should start from explicit product requirements, not this deleted UI
Tested: cd crates/desktop && pnpm test && pnpm build
Not-tested: Full Tauri app manual run"
```

### Task 6: Run final cross-workspace verification and cleanup leftovers

**Files:**
- Modify: any leftover approval references found by search
- Modify: `Cargo.lock` if dependency graph changed

**Step 1: Search for leftovers**

Run:

```bash
rg -n "approval|WaitingForApproval|ApprovalResolved|resolve_approval|pendingApprovalRequest" .
```

Expected:

- no live code references remain
- only intentional historical text, if any, remains and should be reviewed

**Step 2: Run final verification**

Run:

```bash
cargo test -p argus-protocol
cargo test -p argus-agent
cargo test -p argus-wing
cd crates/desktop && pnpm test && pnpm build
```

**Step 3: Commit final cleanup**

```bash
git add Cargo.lock .
git commit -m "Finish removing the legacy approval slice

Constraint: Redesign work needs a clean baseline without approval-era seams
Rejected: Stop after compile fixes | would leave dead references and misleading tests behind
Confidence: high
Scope-risk: moderate
Directive: Redesign human-in-the-loop flow from current requirements instead of restoring deleted types
Tested: cargo test -p argus-protocol; cargo test -p argus-agent; cargo test -p argus-wing; cd crates/desktop && pnpm test && pnpm build
Not-tested: Manual desktop runtime exercise"
```
