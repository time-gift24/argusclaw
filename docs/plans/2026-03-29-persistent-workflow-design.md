# Persistent Workflow Design

## Summary

Build a persistent workflow system that lets the main agent instantiate a fixed workflow template, append extra nodes at instantiation time, and then track the workflow as one durable execution.

The design should maximize reuse of the existing job model and execution path. In v1, workflow execution nodes are materialized into persisted `jobs`, and the main agent interacts with workflow-level APIs instead of directly managing individual background jobs.

## Background

The repository already has two partial building blocks:

- Runtime job orchestration for background subagent work
- Persistence primitives for `workflows` and `jobs`

Today these parts are not connected into a durable workflow system. The current runtime can dispatch subagent jobs and surface progress in the UI, but the execution graph itself is not persisted in a way that lets the system reliably recover and continue after restart.

The desired product behavior is:

- Workflow templates are persisted
- Templates contain workflow nodes and the agent assignment for each node
- When the main agent instantiates a workflow, it may append extra nodes onto the existing template graph
- This graph mutation is only allowed during instantiation
- After the workflow starts, the main agent only observes progress and results

## Goals

- Persist reusable workflow templates
- Persist workflow executions as durable runtime state
- Allow append-only node expansion during workflow instantiation
- Reuse the existing job dependency, status, result, and grouping capabilities as much as possible
- Give the main agent a workflow-level interface focused on start and progress inspection
- Recover pending or running workflows after restart

## Non-Goals

- Editing workflow structure after execution has started
- Deleting or rewriting template nodes during instantiation
- Compensation logic or rollback flows
- Complex retry policies in v1
- Human recovery dashboards in v1
- Event-sourced workflow reconstruction

## Selected Approach

Use a two-layer model:

1. `WorkflowTemplate`
   Stores the reusable workflow graph definition.
2. `WorkflowExecution`
   Stores one concrete run of a template.

At instantiation time, the system materializes a complete execution graph by combining:

- Template nodes
- Main-agent-supplied extra nodes
- Final dependency relationships

That concrete execution graph is then persisted primarily as grouped `jobs`.

This approach is intentionally simple:

- Templates remain stable and reusable
- Executions are explicit and auditable
- The scheduler operates on concrete persisted jobs instead of resolving overlays at runtime
- Existing `jobs` repository logic can be reused directly

## Why Not Other Approaches

### Template Plus Overlay Patch

An overlay model would store only the delta from the template and resolve the final graph on each read or scheduling pass. That reduces duplication, but makes query logic, recovery, and debugging significantly more complex.

### Event Sourcing

An event log could reconstruct workflow state from instantiation and transition events. That provides a strong audit trail, but is too heavy for the current need and does not align with the goal of reusing the existing job model quickly.

## Architecture

Introduce a thin `WorkflowManager` that orchestrates template instantiation and workflow progress, while delegating actual node execution to the existing job system.

Responsibilities of `WorkflowManager`:

- Create and version workflow templates
- Instantiate a workflow execution from a template plus append-only node additions
- Materialize execution nodes into persisted jobs
- Trigger ready-job dispatch for a workflow execution
- Aggregate job state into workflow-level progress

Responsibilities that remain with the existing job layer:

- Ready-job lookup
- Job persistence
- Job execution
- Job result storage
- Job status transitions

This keeps `workflow` as a durable orchestration layer on top of `job`, rather than introducing a second execution system.

## Data Model

### Workflow Templates

Add template persistence tables:

- `workflow_templates`
  - `id`
  - `name`
  - `version`
  - `description`
  - timestamps
- `workflow_template_nodes`
  - `template_id`
  - `node_key`
  - `name`
  - `agent_id`
  - `prompt`
  - `context`
  - `depends_on_keys`

`depends_on_keys` may be stored as a JSON array in v1 to mirror the existing `jobs.depends_on` shape and simplify graph materialization.

Template versioning should be immutable:

- Editing a template creates a new version
- Executions always bind to one exact template version

### Workflow Executions

Reuse the existing `workflows` table as the workflow execution header, extending it with:

- `template_id`
- `template_version`
- `initiating_thread_id`
- optional execution input or metadata payload

The existing workflow status enum is sufficient for v1:

- `pending`
- `running`
- `succeeded`
- `failed`
- `cancelled`

### Execution Nodes via Jobs

Reuse the existing `jobs` table as the execution-node store.

Each workflow node becomes one persisted job with:

- `job_type = workflow`
- `group_id = workflow_execution_id`
- `agent_id`
- `prompt`
- `context`
- `depends_on`
- `status`
- `result`
- timing fields

Add one minimal new column:

- `node_key`

This creates a stable logical node identifier within a workflow execution while still allowing the system to use the job row ID as the execution-time dependency target.

Add a uniqueness constraint on:

- `(group_id, node_key)`

This allows:

- direct progress lookup by logical node
- deterministic mapping from template node or appended node to persisted job row

## Instantiation Semantics

Instantiation is append-only in v1.

Allowed:

- add new nodes
- connect new nodes to existing template nodes
- connect new nodes to other new nodes

Not allowed:

- delete template nodes
- rewrite template node agent assignments
- mutate template node prompts or dependency structure

This keeps the model easy to reason about and avoids turning instantiation into a full graph editor.

## Instantiation Flow

1. Load one immutable workflow template version.
2. Accept a main-agent-supplied patch containing extra nodes.
3. Validate:
   - node keys are unique
   - referenced dependencies exist
   - referenced agents exist
   - the final graph is acyclic
4. Start a transaction.
5. Insert one workflow execution row.
6. Build the full execution graph from:
   - template nodes
   - appended nodes
7. Allocate job IDs for all nodes.
8. Insert one job row per node with:
   - `group_id = workflow_execution_id`
   - resolved `depends_on = upstream job ids`
9. Commit the transaction.
10. Trigger ready-job dispatch for this workflow execution.

After this point the execution graph is frozen.

## Scheduling Model

Do not introduce a heavy new scheduler in v1.

Use an event-driven plus recovery-scan model:

- Immediately after workflow instantiation, dispatch ready jobs
- After any workflow job completes, dispatch newly unlocked ready jobs in the same workflow
- On application startup, scan unfinished workflows and attempt ready-job dispatch again

The scheduler path for a workflow execution should:

1. Find ready jobs belonging to the execution
2. Move them from `pending` to `running`
3. Execute them through the existing `JobManager`
4. Persist results and terminal status
5. Recompute the workflow execution aggregate status

## Workflow Status Rules

Keep workflow status semantics intentionally simple in v1:

- `pending`
  - execution exists but no node has started yet
- `running`
  - at least one job is running, or unfinished jobs remain
- `succeeded`
  - all jobs succeeded
- `failed`
  - any job failed
- `cancelled`
  - reserved for later work

This avoids retries, partial-success semantics, and compensation logic in the first version.

## Recovery Model

Because workflows are persistent, recovery behavior matters more than exact one-time job semantics in v1.

Startup recovery should:

- scan workflow executions in `pending` or `running`
- inspect their grouped jobs
- re-drive ready-job dispatch

For jobs left in `running` with no completion timestamp after a process restart, the safest v1 policy is:

- reset them to `pending`
- allow them to be redispatched

This favors eventual progress and operational simplicity over exact-once execution guarantees. If stronger delivery guarantees are needed later, lease or heartbeat semantics can be layered on top.

## Main-Agent Interface

The main agent should interact with workflows at the execution level, not at the individual job level.

Recommended workflow-facing operations:

- `instantiate_workflow(template_id, extra_nodes, input)`
  - returns `workflow_execution_id`
- `get_workflow_progress(workflow_execution_id)`
  - returns aggregate workflow status and node summaries
- `list_workflow_runs(thread_id | session_id)`
  - returns workflow executions initiated from a conversational scope
- `get_workflow_details(workflow_execution_id)`
  - returns the full execution graph and node results

If exposed as tools for the main agent, keep the surface area minimal:

- `start_workflow`
- `get_workflow_progress`

That keeps the main agent behavior stable:

- choose template
- append nodes during instantiation
- start execution
- watch progress

## Progress Model

Workflow progress should be computed from grouped jobs.

`get_workflow_progress` should return:

- workflow execution ID
- template ID and version
- overall workflow status
- total node count
- pending count
- running count
- succeeded count
- failed count
- current running nodes
- per-node summary:
  - `node_key`
  - `name`
  - `agent_id`
  - `status`
  - `started_at`
  - `finished_at`
  - short result summary if available

This allows both the UI and the main agent to focus on one workflow object instead of a loose set of job IDs.

## Error Handling

V1 error handling should stay strict and predictable:

- invalid graph at instantiation time fails before any writes commit
- any node execution failure marks that job failed
- any failed job marks the workflow failed
- no automatic retries in v1
- no compensating actions in v1

This makes the first release easy to observe and debug.

## Testing Strategy

Test coverage should focus on four layers:

### Repository Tests

- template persistence
- execution header persistence
- grouped job lookup and aggregation

### Instantiation Tests

- template plus appended nodes materialize correctly
- dependency resolution maps node keys to job IDs correctly
- invalid graphs fail validation

### Scheduling Tests

- ready jobs are dispatched in dependency order
- downstream nodes unlock when upstream nodes succeed
- workflow status changes from pending to running to succeeded
- workflow fails when any job fails

### Recovery Tests

- pending and running workflows are rediscovered on startup
- orphaned running jobs are reset to pending and resumed

## Rollout Constraints

Keep v1 intentionally narrow:

- no runtime graph mutation after execution start
- no template-node overrides during instantiation
- no cancellation semantics beyond stored status support
- no retries
- no compensation
- no human recovery console

The success condition for v1 is narrower and more valuable:

Persist a workflow template, instantiate it into a durable execution with append-only startup customization, execute it via grouped jobs, and let the main agent observe progress through one workflow execution handle.
