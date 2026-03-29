# Chrome Explore Tool Design

## Summary

Build a read-only Chrome capability for ArgusWing that can power an explore agent with hard action whitelisting.

The implementation will live under `crates/argus-tool/src/chrome/` as a subdirectory-based module, while exposing a single `chrome` tool to agents through the existing `tool_names` model.

## Background

We evaluated [`Ulyssedev/Rust-undetected-chromedriver`](https://github.com/Ulyssedev/Rust-undetected-chromedriver) as a reference.

Useful takeaway:
- The library demonstrates a workable `chromedriver` patching approach for reducing automation fingerprinting.

Reasons not to adopt it directly:
- Its published crate version is old and depends on outdated libraries such as `thirtyfour = 0.31.0` and `reqwest = 0.11.18`.
- Its core logic is concentrated in a single `src/lib.rs`.
- Production code contains many `unwrap()` / `expect()` calls.
- Downloaded binaries are written into the current working directory instead of an Argus-managed cache root.

Decision:
- Do not depend on the library directly.
- Reuse only the general patching idea where it still makes sense.
- Reimplement download, cache layout, error handling, and policy enforcement in Argus style.

## Goals

- Add a Chrome-backed tool that can support a read-only explore agent.
- Enforce allowed actions in code, not just in prompts.
- Store all Chrome-related assets under `~/.arguswing/chrome`.
- Avoid production `unwrap()` / `expect()`.
- Keep the external agent-facing surface area small: one tool, multiple whitelisted actions.

## Non-Goals

- General browser automation.
- Form filling, clicking, or submission workflows.
- Arbitrary JavaScript execution.
- Arbitrary download or upload behavior.
- Arbitrary output paths for screenshots or driver binaries.

## High-Level Approach

Expose one `chrome` tool, but structure it internally in three layers:

1. `ChromeManager`
   Owns driver discovery, download, patching, local process lifecycle, and cache reuse.
2. `ChromeSession`
   Owns one browser session and wraps read-only `thirtyfour` interactions.
3. `ExplorePolicy`
   Owns the hard whitelist for allowed actions and argument constraints.

This keeps the public tool API simple while leaving room for future expansion without rewriting the base.

## Module Layout

Create a new subdirectory at `crates/argus-tool/src/chrome/` with the following files:

- `mod.rs`
- `tool.rs`
- `manager.rs`
- `session.rs`
- `policy.rs`
- `installer.rs`
- `patcher.rs`
- `models.rs`
- `error.rs`

Integration updates:
- `crates/argus-tool/src/lib.rs`
  Add `pub mod chrome;` and re-export `ChromeTool`.
- `crates/argus-wing/src/lib.rs`
  Register `ChromeTool` alongside the default built-in tools when enabled for the application.

## Tool Contract

The agent sees one tool named `chrome`.

The tool accepts a constrained action-based schema:

```json
{
  "action": "open | wait | extract_text | list_links | get_dom_summary | screenshot",
  "session_id": "optional",
  "url": "required for open",
  "timeout_ms": 10000,
  "selector": "optional for scoped read actions",
  "format": "optional for extract_text"
}
```

Validation strategy:
- Deserialize into strict tool args with unknown fields rejected.
- Validate by action after deserialization.
- Return explicit authorization or argument errors for unsupported combinations.

## Whitelist Policy

The first version is intentionally read-only.

Allowed actions:
- `open`
- `wait`
- `extract_text`
- `list_links`
- `get_dom_summary`
- `screenshot`

Denied actions:
- `click`
- `type`
- `submit`
- `execute_script`
- Cookie or storage mutation
- Arbitrary file download
- Arbitrary file upload
- Window or tab mutation beyond what is required for read-only navigation

Policy layers:

### ActionAllowList

Only the approved six actions are recognized.

### ActionGuard

Each allowed action has extra constraints:
- `open` can navigate only.
- `wait` can only wait for page readiness or selector presence.
- `extract_text`, `list_links`, and `get_dom_summary` can only read page state.
- `screenshot` can only write into the managed screenshot directory.

### SessionGuard

Session state stays read-oriented:
- current URL
- page title
- last screenshot path
- cached DOM summary metadata

### OutputGuard

Outputs are bounded to avoid token explosion and accidental leakage:
- extracted text length capped
- link count capped
- DOM summary returned as a compressed summary, not raw HTML
- screenshots returned as paths plus metadata, not auto-inlined binary content

## Origin Policy

Although the immediate requirement is action whitelisting, the design should reserve an origin policy layer.

Proposed initial behavior:
- Allow non-local `http` and `https` targets by default.
- Reject local addresses or other unsafe destinations if they conflict with broader tool security rules.

Future hardening path:
- Swap in an explicit allowlist of domains or origins without changing the tool contract.

## Session and Data Flow

### `open`

1. `ChromeTool` validates action and arguments.
2. `ExplorePolicy` confirms the action is allowed.
3. `ChromeManager` ensures cached driver resources exist under `~/.arguswing/chrome`.
4. A `ChromeSession` is created or reused.
5. The session navigates to the target URL.
6. The tool returns `session_id`, page title, and final URL.

### `wait`

Allows only passive waiting:
- document readiness
- bounded timeout
- optional selector appearance

### `extract_text`

Reads visible page text:
- full document text by default
- scoped text when a selector is supplied

### `list_links`

Returns a bounded list of visible links with metadata such as:
- text
- href
- same-origin status

### `get_dom_summary`

Returns a compact structural summary suitable for agent reasoning instead of raw HTML.

### `screenshot`

Writes a screenshot into the managed output directory and returns path metadata.

## Cache and Filesystem Layout

All managed assets live under `~/.arguswing/chrome`:

- `driver/`
  Raw download artifacts and extracted original driver binaries.
- `patched/`
  Patched driver binaries used at runtime.
- `screenshots/`
  Managed screenshot output directory.
- `tmp/`
  Temporary download and extraction workspace.

Constraints:
- Callers cannot choose the driver install path.
- Callers cannot choose arbitrary screenshot output paths.
- Install and patch operations must be serialized with a lock to prevent concurrent corruption.
- Patch metadata should be recorded so compatible binaries can be reused.

## Error Handling

Define internal errors in `crates/argus-tool/src/chrome/error.rs`, then map them to `ToolError` at the outer tool boundary.

Suggested internal error variants:
- `UnsupportedPlatform`
- `ChromeNotInstalled`
- `ChromeVersionDetectFailed`
- `DriverDownloadFailed`
- `DriverArchiveInvalid`
- `DriverPatchFailed`
- `DriverStartFailed`
- `SessionNotFound`
- `ActionNotAllowed`
- `InvalidActionArgs`
- `NavigationFailed`
- `PageReadFailed`
- `ScreenshotFailed`

Requirements:
- No production `unwrap()` / `expect()`.
- Preserve context when mapping lower-level errors.
- Make retryable versus non-retryable failures distinguishable in logs and diagnostics.

## Testing Strategy

Use three layers of tests while keeping CI stable and mostly offline-safe.

### Unit Tests

Cover:
- action whitelist enforcement
- argument validation
- path policy
- output truncation
- error mapping

### Component Tests

Use fake installer and fake session adapters to test:
- `tool -> policy -> session` flow
- session lookup and reuse
- rejection of denied actions

### Optional Smoke Tests

Only run when explicitly enabled and when local Chrome is available:
- driver bootstrap
- patched driver startup
- one read-only navigation flow

Default CI should not require:
- live driver downloads
- real browser startup
- external site availability

## Agent Integration

The intended agent model is:
- the explore agent template exposes only the `chrome` tool
- the tool itself enforces read-only capability
- approval policy can still treat `chrome` as a high-risk tool if desired

This means template-level visibility and tool-level capability limits work together:
- template controls whether an agent can use Chrome at all
- `ExplorePolicy` controls what the Chrome tool is permitted to do

## Open Design Decisions for Implementation Planning

- Whether `ChromeManager` should manage a shared driver process or start one per session.
- Whether screenshots should return absolute local paths or a more abstract resource handle.
- Which exact `thirtyfour` version best matches the workspace dependency strategy.
- Whether the first version should register `chrome` in all default tool sets or only for explicit explore-agent templates.

## Recommended Implementation Direction

Implement the single-tool approach, but keep the internal layering close to a future browser service architecture.

That gives us:
- a simple tool surface for the agent
- strong code-enforced read-only behavior
- an Argus-owned cache and patch pipeline
- a clean base for later adding stricter origin allowlists or richer browser capabilities
