# chat-plan-display

## ADDED Requirements

### Requirement: Plan panel appears immediately when update_plan tool starts

The system SHALL display the Plan panel immediately when the `update_plan` tool starts execution (via `tool_started` event), extracting the initial plan from the tool's arguments.

#### Scenario: Plan panel appears on tool_started
- **WHEN** a `tool_started` event arrives with `tool_name === "update_plan"`
- **THEN** the Plan panel is rendered below the reasoning block in the current pending assistant message

#### Scenario: Plan panel initialized from tool arguments
- **WHEN** a `tool_started` event arrives with `tool_name === "update_plan"`
- **THEN** the initial plan state is extracted from the `arguments` field of the tool call (field: `plan: PlanItem[]`)

### Requirement: Plan panel updates in real-time as tasks complete

The system SHALL update the Plan panel display whenever task status changes occur, reflecting the latest `pendingAssistant.plan` state.

#### Scenario: Task status update reflected immediately
- **WHEN** a `tool_completed` event arrives for any tool and the plan state has changed
- **THEN** the Plan panel re-renders with the updated task statuses (pending / in_progress / completed)

#### Scenario: Plan panel shows current completion count
- **WHEN** the Plan panel is displayed
- **THEN** it shows the completion count in the format `(completed/total)` in the header

### Requirement: Plan panel positioned below reasoning block

The system SHALL render the Plan panel as part of the assistant message, immediately below the reasoning block and above the message footer/action bar.

#### Scenario: Plan panel layout order
- **WHEN** an assistant message with plan is rendered
- **THEN** the rendering order is: main text → reasoning block → plan panel → action bar

### Requirement: Plan panel hidden when no plan exists

The system SHALL NOT render the Plan panel when the current session has no active plan (plan is null or empty).

#### Scenario: No plan means no panel
- **WHEN** `session.pendingAssistant.plan` is `null` or an empty array
- **THEN** the Plan panel is not rendered
