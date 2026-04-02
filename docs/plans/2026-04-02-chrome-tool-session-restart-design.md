# Chrome Tool Session Restart Design

**Date:** 2026-04-02

## Goal

Clarify that the `chrome` tool operates on a shared browser session in production mode and add explicit `close` and `restart` actions so agents can either close the active session or force a clean browser restart.

## Current Behavior

- `chrome.open` already behaves like a shared-session entry point in production mode.
- `ChromeManager` is configured with a single-session limit in production, so a second `open` reuses the existing session and performs a `navigate`.
- `SystemChromeHost` also reuses a shared chromedriver process when the driver binary matches.
- None of this is visible in the tool definition or the `Chrome Explore` agent prompt.
- There is no explicit tool action that lets an agent close the shared session or force a full restart of the shared browser runtime.

## Desired Behavior

### Shared Session Semantics

- Tool descriptions and agent guidance should say that `open` uses a shared session.
- In production mode, repeated `open` calls should continue to reuse the current session and navigate the active tab instead of creating a second session.

### `close` Action

- Add `close` as a tool action that requires `session_id`.
- `close` shuts down the browser session tracked by that `session_id` and removes it from the manager.
- `close` does not promise a full browser runtime reset; it is a lightweight session shutdown.

### `restart` Action

- Add `restart` as a tool action that requires `session_id` and `url`.
- `restart` closes the tracked session, clears any remaining tracked sessions, resets the shared chromedriver process when the managed production host is in use, and then opens a fresh session at the requested URL.
- The response shape should mirror `open` enough for agents to keep working with the returned `session_id`, `final_url`, and `page_title`.

## API Contract

### Tool Definition Copy

- Read-only description should mention:
  - explicit driver install
  - shared session reuse by `open`
  - `close` for shutting down the active session
  - `restart` for forcing a clean browser restart
- Interactive description should carry the same session semantics plus the existing interactive guidance.

### Validation Rules

- `close` accepts only `session_id`.
- `restart` accepts only `session_id` and `url`.
- `restart.url` follows the same validation rules as `open` and `navigate`.

## Implementation Notes

- Extend `ChromeAction`, `ChromeToolArgs`, tool definition enums, and validators.
- Add `ChromeManager::restart` and a shared-host reset helper.
- Reuse existing `close_session`, `open`, and host shutdown logic instead of duplicating shutdown code.
- Keep the manager behavior single-session in production; this change only makes the behavior explicit and controllable.

## Testing

- Add validation tests for `close` and `restart`.
- Add tool-definition tests asserting the new actions are exposed.
- Add manager/tool tests covering:
  - `close` removes the session and shuts it down
  - `restart` returns a fresh session id
  - `restart` resets the shared host when managed support is present
- Update agent prompt snapshot expectations if needed.
