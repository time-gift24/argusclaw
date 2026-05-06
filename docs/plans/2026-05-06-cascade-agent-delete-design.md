# Cascade Agent Delete Design

## Goal

Deleting an agent template should stay safe by default. If the template is still referenced by sessions, threads, or jobs, the existing delete path continues to block deletion. Users may explicitly choose a cascade option that deletes the agent's associated jobs and threads, then removes any sessions left empty by that cleanup.

This design covers the core Rust layers, Tauri desktop bridge, server API, desktop React UI, and Web Vue UI.

## Decisions

- Default delete remains non-cascading and reference-blocked.
- Cascade delete is opt-in through a clear `cascade_associations` / `cascadeAssociations` flag.
- Mixed sessions are preserved. Only threads whose `template_id` matches the deleted agent are removed; a session is deleted only after it has no remaining threads.
- `subagent_names` references still block deletion, even with cascade enabled. Those are configuration references, not historical runtime data.
- SQL stays in `argus-repository`; higher layers call narrow repository or manager APIs.

## Backend Architecture

`TemplateManager::delete(id)` keeps its current behavior. A new explicit API, for example `delete_with_options(id, TemplateDeleteOptions { cascade_associations })`, selects the delete mode.

When `cascade_associations` is false, the manager uses the existing reference count checks and error text.

When it is true, the manager still checks the target agent and `subagent_names` references. If another agent lists the target display name in `subagent_names`, deletion is blocked. If not, it calls a repository-level transaction method such as `delete_with_associations(&AgentId) -> AgentDeleteReport`.

The report should include at least:

- `agent_deleted: bool`
- `deleted_job_ids: Vec<JobId>` or `deleted_job_count`
- `deleted_thread_ids: Vec<ThreadId>` or `deleted_thread_count`
- `deleted_session_ids: Vec<SessionId>` or `deleted_session_count`

Returning IDs is preferable for desktop/session facade cleanup; counts are enough for UI feedback.

## Repository Transaction

The repository performs the cascade delete in one transaction:

1. Select threads with `threads.template_id = agent_id`, including their `thread_id` and `session_id`.
2. Delete jobs with `jobs.agent_id = agent_id`.
3. Delete selected threads. Existing message cleanup should continue to rely on the thread/message cascade or the repository's current thread delete semantics.
4. For the touched session IDs, delete only sessions that now have zero threads.
5. Delete the agent row. MCP bindings already use FK cascade and should continue to follow the agent row.

If any step fails, the transaction rolls back. SQLite and Postgres implementations should expose the same trait contract.

## Facades and Transports

`argus-wing` adds a delete-template method that accepts options and delegates to `TemplateManager`.

`crates/desktop/src-tauri` updates `delete_agent_template` to accept an optional `cascade_associations` argument while keeping the old no-option call working.

`argus-server` updates:

- `ServerCore::delete_template(id, options)`
- `DELETE /api/v1/agents/templates/{template_id}?cascade_associations=true`

The HTTP route should avoid DELETE request bodies. The response should keep the existing mutation envelope and include the delete report. Non-cascade calls can return the same report shape with zero associated deletions.

## Desktop UI

The current settings agents page keeps its first confirmation dialog. The first delete attempt calls `agents.delete(id)` without cascade.

If the backend returns the reference-blocked error, the page opens a second confirmation state that explains the agent still has associated jobs or conversation threads and offers a destructive action such as "同时删除关联数据".

On confirmation, the page calls `agents.delete(id, { cascadeAssociations: true })`. After success it reloads the list and shows a Chinese success message based on the report:

- `模板已删除。`
- `模板已删除，并清理 X 个任务、Y 个线程、Z 个空会话。`

The frontend binding in `crates/desktop/lib/tauri.ts` becomes:

```ts
delete: (id: number, options?: { cascadeAssociations?: boolean }) =>
  invoke<AgentDeleteReport>("delete_agent_template", {
    id,
    cascadeAssociations: options?.cascadeAssociations ?? false,
  })
```

## Web UI

`apps/web/src/lib/api.ts` changes `deleteTemplate` to accept options:

```ts
deleteTemplate(templateId: number, options?: { cascadeAssociations?: boolean }): Promise<AgentDeleteReport>
```

The server client sends:

```ts
DELETE /api/v1/agents/templates/:id?cascade_associations=true
```

`TemplatesPage.vue` keeps list ownership. It should:

1. Click "删除模板" and attempt non-cascade delete.
2. If the backend reports references, show an OpenTiny confirmation dialog.
3. On confirm, call `deleteTemplate(id, { cascadeAssociations: true })`.
4. Refresh the template list.
5. Show the same report-based Chinese success message as desktop.

The page should not introduce a new route or shared desktop store. Web remains independent and uses `src/lib/api.ts` as its server contract boundary.

## Error Handling

- Missing agent follows the existing delete semantics where possible.
- `subagent_names` references return a blocking error in both modes.
- Database FK failures should be mapped to the same database error style used today.
- Cascade failures roll back all deletions.
- Running jobs or loaded thread runtimes are not separately interrupted in this first version. If higher layers need in-memory cleanup, they should use the report's deleted IDs and call existing runtime removal paths after the transaction succeeds.

## Testing

Repository tests:

- Non-cascade delete still blocks when jobs or threads reference the agent.
- Cascade delete removes `jobs.agent_id = agent_id`.
- Cascade delete removes matching threads and their messages.
- Mixed sessions remain when other threads still exist.
- Sessions containing only deleted threads are removed.
- `subagent_names` references block even with cascade enabled.
- SQLite is required. Postgres should get matching coverage where the current harness supports it.

Manager/facade tests:

- `TemplateManager::delete` preserves existing behavior.
- `delete_with_options(... cascade_associations: true)` returns an accurate report.
- `argus-wing` and `argus-server` pass the option through without adding business logic.

Frontend tests:

- Desktop binding sends `cascadeAssociations` only when requested.
- Web API client encodes `cascade_associations=true` in the query string.
- Web templates page first tries normal delete.
- On a reference-blocked error, the page shows cascade confirmation and retries with cascade after user approval.
- Success messages include cleanup counts when present.

## Implementation Notes

Keep names explicit. Prefer `cascade_associations` over `force`, because this flag deletes a defined set of associated runtime data rather than bypassing every safety check.

Do not change FK defaults to `ON DELETE CASCADE` for jobs or threads. The product behavior is an application-level explicit operation, not a database default.
