# Session-Owned Thread Mailbox Routing Design

## Summary

Replace the current mixed control-plane model with a simpler routing model:

- `Session` owns the mailbox mapping for loaded threads in the session.
- All inbound thread work is routed through that session-owned mailbox table.
- `Thread` consumes its own mailbox and owns runtime state directly.
- `Turn` no longer reads shared mailbox state and no longer owns `control_tx`.
- `interrupt` remains a pure stop signal and does not carry redirect text.

This design intentionally avoids introducing extra abstraction layers such as a separate
`ThreadMailboxRegistry`, `ThreadRouterHandle`, or a standalone `ThreadReactor`.

## Goals

- Make `Session` the single owner of thread inbound routing.
- Collapse the current split between `ThreadInbox`, `ThreadMailbox`, `ThreadControlEvent`,
  and `control_tx` into one authoritative inbound path.
- Keep FIFO semantics for queued user messages and mailbox messages.
- Keep `interrupt` semantics simple: stop the active turn only.
- Remove the current dual-consumer problem where both `Thread` and `Turn` can observe mailbox state.
- Preserve outbound event broadcasting via `thread_event_tx` and `stream_tx`.

## Non-Goals

- Redesign `ThreadEvent` or `TurnStreamEvent` broadcasting.
- Add redirect-capable interrupts.
- Introduce a generic routing/registry layer just for abstraction purity.
- Rework desktop/frontend event semantics beyond whatever is needed to preserve current behavior.

## Current Problems

The current implementation carries several overlapping concepts:

- `ThreadInbox` is already the real FIFO queue for queued user messages and mailbox messages.
- `ThreadMailbox` wraps `ThreadInbox` and also carries legacy interrupt state.
- `Thread` still owns `control_tx` / `control_rx` and drives a separate `ThreadReactor` state machine.
- `Turn` still receives both `mailbox` and `control_tx`, even though it should only execute the current turn.
- Running turns can call `mailbox.drain_for_turn()` before each loop iteration, which means inbound state is not owned exclusively by the thread runtime.

This creates confusing ownership:

- `Session` wants to route work to a thread.
- `Thread` wants to own execution order.
- `Turn` can still read shared inbound state directly.

That overlap makes behavior harder to reason about, especially for:

- queued follow-up messages while a turn is running
- job result handoff
- stop/interrupt behavior
- future runtime unload/load handling

## Proposed Design

### 1. Session Owns Thread Mailboxes

Each loaded thread inside a session gets one session-owned mailbox entry, keyed by `ThreadId`.

`Session` / `SessionManager` becomes the only place responsible for routing inbound work to a target thread:

- enqueue user message
- enqueue mailbox message
- interrupt a running thread
- claim a queued job result
- inspect unread mailbox messages
- mark mailbox messages as read

This does not require a new named abstraction type. The mailbox map can live directly inside the existing session/session-manager structures.

### 2. ThreadMailbox Becomes the Authoritative Inbound Queue

`ThreadMailbox` should become the only thread-local inbound queue abstraction.

Its responsibilities are:

- hold queued `UserMessage`
- hold queued `MailboxMessage`
- preserve global FIFO order across those queued inputs
- support `claim_job_result(job_id)` without disturbing the order of unrelated items
- support unread/read behavior for queued mailbox messages

`interrupt(stop)` is intentionally not modeled as queued user input. It is a control signal that targets the currently active turn only.

That means the mailbox model becomes:

- queued FIFO inputs:
  - user messages
  - mailbox messages
- stop signal:
  - immediate control action, not queued for the next turn

The current compatibility shape:

- `ThreadInbox`
- `ThreadMailbox { user_interrupts, inbox }`

should be collapsed into one simpler mailbox model so that the code no longer implies two different queue owners.

### 3. Thread Owns Runtime State Directly

`ThreadReactor` should be removed and its state machine logic folded back into `Thread`.

`Thread` should directly own:

- current runtime state
- next turn number
- queue depth
- active turn cancellation handle
- mailbox consumption loop

The thread loop should do two things:

- respond to mailbox arrivals
- respond to active turn progress / settlement

There should no longer be a separate `ThreadReactorAction` indirection. `Thread` can decide directly whether to:

- start the next turn
- request cancellation of the active turn
- stay idle

This keeps the ownership boundary clear:

- `Session` routes inputs into the target thread mailbox
- `Thread` decides when those inputs become execution
- `Turn` executes only the current turn

### 4. Turn Stops Owning Inbound Routing

`Turn` should keep only execution-local and outbound concerns:

- `stream_tx`
- `thread_event_tx`
- `cancellation`

`Turn` should drop:

- `control_tx`
- `mailbox`

`TurnCancellation` remains necessary and should stay. It is not an alternate routing path; it is the concrete execution primitive used to stop an active turn after the thread runtime receives an interrupt.

The distinction is:

- `interrupt(stop)` = external intent addressed to a thread
- `TurnCancellation` = local execution mechanism that actually stops the running turn

### 5. Inbound Runtime Semantics

The authoritative runtime behavior should be:

#### Idle thread

- `UserMessage` / `MailboxMessage`:
  - enqueue into mailbox FIFO
  - immediately start the next turn from queue head
- `interrupt(stop)`:
  - no-op

#### Running or WaitingForApproval thread

- `UserMessage` / `MailboxMessage`:
  - enqueue only
  - do not affect the current active turn
- `interrupt(stop)`:
  - transition to stopping state
  - call `TurnCancellation.cancel()` on the active turn

#### Stopping thread

- additional `interrupt(stop)`:
  - no-op
- additional queued inputs:
  - enqueue and wait

#### After turn settlement

- if queued work exists, dequeue the next FIFO item and start the next turn
- otherwise transition to idle

The running turn should no longer drain mailbox inputs itself. That legacy compatibility path should be removed so the thread is the only consumer of inbound queue state.

### 6. Tool and Job Routing

`ToolExecutionContext` should stop exposing `control_tx`.

Instead, scheduler behavior should rely on session-owned routing by `thread_id`:

- dispatching jobs
- consuming queued job results
- sending mailbox messages

The practical effect is:

- the scheduler tool still receives the current `thread_id`
- the backend uses session-owned routing APIs instead of a thread-internal sender

This keeps routing policy in the orchestration layer rather than pushing thread internals into tool execution context.

### 7. Failure Semantics

The design keeps failure behavior explicit:

- route to missing `ThreadId`: return an error
- route to unreachable thread: return an error
- route to non-ready job thread: return an error
- `interrupt(stop)` on idle thread: no-op
- `interrupt(stop)` on running/waiting thread: cancel active turn only
- queued FIFO inputs survive cancellation and are considered after settlement
- `claim_job_result(job_id)` removes only the matching queued job result
- unread/read applies only to queued mailbox messages

## Migration Strategy

This change should be implemented incrementally:

1. Simplify mailbox data model and lock semantics down with tests.
2. Move routing entrypoints into `Session` / `SessionManager`.
3. Inline runtime state machine logic back into `Thread`.
4. Remove `Turn.mailbox` and `Turn.control_tx`.
5. Update scheduler/job manager paths to use session-owned routing.
6. Delete legacy compatibility code that is no longer used.

The order matters because tests should protect semantics before internal control paths are removed.

## Testing Strategy

Coverage should include:

- FIFO ordering across user messages and mailbox messages
- `interrupt(stop)` as a no-op while idle
- `interrupt(stop)` cancelling an active turn without becoming next-turn input
- queued FIFO work surviving cancellation and starting in order after settlement
- claiming a queued job result while preserving remaining order
- unread/read behavior for mailbox messages
- scheduler/job result flows still routing through the originating thread correctly
- runtime state transitions:
  - `Idle -> Running`
  - `Running/WaitingForApproval -> Stopping`
  - `Stopping -> Idle` or `Stopping -> Running(next)`

## Expected Outcome

After this refactor:

- inbound thread routing has one owner: `Session`
- execution has one owner: `Thread`
- active turn execution has one owner: `Turn`
- interrupt behavior is easier to explain and test
- queue semantics stop depending on legacy compatibility drains
- the codebase loses one unnecessary layer (`ThreadReactor`) instead of gaining more
