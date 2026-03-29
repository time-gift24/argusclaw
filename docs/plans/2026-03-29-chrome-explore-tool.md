# Chrome Explore Tool Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a read-only `chrome` tool plus a builtin explore-agent template that can browse with a hard action whitelist.

**Architecture:** Build a new `crates/argus-tool/src/chrome/` submodule with `ChromeManager`, `ChromeSession`, `ExplorePolicy`, and one exposed `ChromeTool`. Store all browser assets under `~/.arguswing/chrome`, register the tool in `argus-wing`, and create an explore-agent template that exposes only `chrome`.

**Tech Stack:** Rust 2024, Tokio, Reqwest 0.12, Thirtyfour 0.36.1, Zip 8.4.0, Serde, ThisError, DashMap.

---

## Execution Preconditions

- Run implementation from a dedicated `.worktrees/...` worktree, not from the root `main` checkout.
- Keep `docs/plans/` changes on `main`; do the Rust implementation in the worktree.
- Follow TDD for each task: write the test first, watch it fail, implement the minimum, rerun, then commit.

### Task 1: Add dependencies, module scaffold, and read-only action policy

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/argus-tool/Cargo.toml`
- Modify: `crates/argus-tool/src/lib.rs`
- Create: `crates/argus-tool/src/chrome/mod.rs`
- Create: `crates/argus-tool/src/chrome/models.rs`
- Create: `crates/argus-tool/src/chrome/policy.rs`
- Create: `crates/argus-tool/src/chrome/error.rs`

**Step 1: Write the failing tests**

Add unit tests that lock down the first hard constraints:

```rust
#[test]
fn open_requires_url() {
    let err = ChromeToolArgs::validate(json!({ "action": "open" })).unwrap_err();
    assert!(err.to_string().contains("url"));
}

#[test]
fn click_is_rejected_by_policy() {
    let err = ExplorePolicy::readonly().validate_action(ChromeAction::Click).unwrap_err();
    assert!(err.to_string().contains("not allowed"));
}

#[test]
fn list_links_is_allowed() {
    ExplorePolicy::readonly()
        .validate_action(ChromeAction::ListLinks)
        .unwrap();
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool open_requires_url click_is_rejected_by_policy list_links_is_allowed
```

Expected:
- FAIL because `chrome` models and policy types do not exist yet.

**Step 3: Write minimal implementation**

Add the dependency and scaffold first:

```toml
# Cargo.toml
[workspace.dependencies]
thirtyfour = { version = "0.36.1", default-features = false, features = ["reqwest", "rustls-tls", "tokio-multi-threaded"] }
zip = "8.4.0"
```

```rust
// crates/argus-tool/src/chrome/models.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeAction {
    Open,
    Wait,
    ExtractText,
    ListLinks,
    GetDomSummary,
    Screenshot,
    Click,
}
```

```rust
// crates/argus-tool/src/chrome/policy.rs
pub struct ExplorePolicy {
    allowed: HashSet<ChromeAction>,
}
```

Implement:
- `ChromeToolArgs` with `#[serde(deny_unknown_fields)]`
- `ChromeAction`
- `ExplorePolicy::readonly()`
- validation errors in `error.rs`
- `pub mod chrome;` in `crates/argus-tool/src/lib.rs`

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool open_requires_url click_is_rejected_by_policy list_links_is_allowed
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add Cargo.toml crates/argus-tool/Cargo.toml crates/argus-tool/src/lib.rs crates/argus-tool/src/chrome
git commit -m "feat: scaffold chrome explore policy"
```

### Task 2: Implement managed cache paths and pure patching helpers

**Files:**
- Create: `crates/argus-tool/src/chrome/installer.rs`
- Create: `crates/argus-tool/src/chrome/patcher.rs`
- Modify: `crates/argus-tool/src/chrome/mod.rs`
- Modify: `crates/argus-tool/src/chrome/error.rs`

**Step 1: Write the failing tests**

Add pure unit tests before any network or process code:

```rust
#[test]
fn chrome_paths_use_arguswing_root() {
    let paths = ChromePaths::from_home(Path::new("/tmp/home"));
    assert_eq!(paths.root, PathBuf::from("/tmp/home/.arguswing/chrome"));
    assert_eq!(paths.screenshots, PathBuf::from("/tmp/home/.arguswing/chrome/screenshots"));
}

#[test]
fn patcher_rewrites_cdc_tokens() {
    let input = b"aaaaacdc_123456789012345678zz".to_vec();
    let output = patch_cdc_tokens(input, b'X').unwrap();
    assert!(!String::from_utf8_lossy(&output).contains("cdc_123456789012345678"));
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool chrome_paths_use_arguswing_root patcher_rewrites_cdc_tokens
```

Expected:
- FAIL because `ChromePaths` and `patch_cdc_tokens` are not implemented.

**Step 3: Write minimal implementation**

Implement:
- `ChromePaths` helper that resolves:
  - `~/.arguswing/chrome/driver`
  - `~/.arguswing/chrome/patched`
  - `~/.arguswing/chrome/screenshots`
  - `~/.arguswing/chrome/tmp`
- directory creation helper
- pure `patch_cdc_tokens` function that mutates byte slices without spawning Chrome

Use small, testable helpers:

```rust
pub struct ChromePaths {
    pub root: PathBuf,
    pub driver: PathBuf,
    pub patched: PathBuf,
    pub screenshots: PathBuf,
    pub tmp: PathBuf,
}
```

```rust
pub fn patch_cdc_tokens(mut bytes: Vec<u8>, fill: u8) -> Result<Vec<u8>, ChromeToolError> {
    // scan for b"cdc_" and overwrite the following marker span
}
```

Do not add download logic yet; keep this task offline and deterministic.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool chrome_paths_use_arguswing_root patcher_rewrites_cdc_tokens
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add crates/argus-tool/src/chrome/mod.rs crates/argus-tool/src/chrome/installer.rs crates/argus-tool/src/chrome/patcher.rs crates/argus-tool/src/chrome/error.rs
git commit -m "feat: add chrome cache paths and patch helpers"
```

### Task 3: Add session adapter and manager with fakeable backend

**Files:**
- Create: `crates/argus-tool/src/chrome/session.rs`
- Create: `crates/argus-tool/src/chrome/manager.rs`
- Modify: `crates/argus-tool/src/chrome/models.rs`
- Modify: `crates/argus-tool/src/chrome/mod.rs`

**Step 1: Write the failing tests**

Write manager tests against a fake backend so CI stays stable:

```rust
#[tokio::test]
async fn manager_creates_session_and_returns_metadata() {
    let backend = Arc::new(FakeBrowserBackend::new("https://example.com", "Example"));
    let manager = ChromeManager::new_for_test(backend);

    let opened = manager.open(OpenArgs { url: "https://example.com".into() }).await.unwrap();

    assert_eq!(opened.final_url, "https://example.com");
    assert_eq!(opened.page_title, "Example");
    assert!(!opened.session_id.is_empty());
}

#[tokio::test]
async fn manager_rejects_unknown_session() {
    let manager = ChromeManager::new_for_test(Arc::new(FakeBrowserBackend::default()));
    let err = manager.session("missing").await.unwrap_err();
    assert!(err.to_string().contains("SessionNotFound"));
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool manager_creates_session_and_returns_metadata manager_rejects_unknown_session
```

Expected:
- FAIL because `ChromeManager`, `ChromeSession`, and the fake backend do not exist.

**Step 3: Write minimal implementation**

Add:
- `ChromeSession` state with:
  - `session_id`
  - `current_url`
  - `page_title`
  - `last_screenshot_path`
- `BrowserBackend` trait for the operations the tool needs
- `ChromeManager` with:
  - session registry
  - `open()`
  - `session()`
  - `list_links()`
  - `extract_text()`
  - `get_dom_summary()`
  - `screenshot()`

Keep the real `thirtyfour` adapter behind the trait boundary so unit tests can stay fake-first.

```rust
#[async_trait]
pub trait BrowserBackend: Send + Sync {
    async fn open(&self, url: &str) -> Result<PageMetadata, ChromeToolError>;
    async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError>;
    async fn list_links(&self) -> Result<Vec<LinkSummary>, ChromeToolError>;
}
```

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool manager_creates_session_and_returns_metadata manager_rejects_unknown_session
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add crates/argus-tool/src/chrome/session.rs crates/argus-tool/src/chrome/manager.rs crates/argus-tool/src/chrome/models.rs crates/argus-tool/src/chrome/mod.rs
git commit -m "feat: add chrome session manager"
```

### Task 4: Implement the `chrome` tool and register it in ArgusWing

**Files:**
- Create: `crates/argus-tool/src/chrome/tool.rs`
- Modify: `crates/argus-tool/src/chrome/mod.rs`
- Modify: `crates/argus-tool/src/lib.rs`
- Modify: `crates/argus-wing/src/lib.rs`

**Step 1: Write the failing tests**

Add tool-level tests that lock the external API:

```rust
#[test]
fn chrome_tool_definition_lists_only_readonly_actions() {
    let tool = ChromeTool::new_for_test(Arc::new(FakeChromeManager::default()));
    let def = tool.definition();
    assert_eq!(def.name, "chrome");
    assert!(def.description.contains("read-only"));
}

#[tokio::test]
async fn chrome_tool_rejects_denied_action_before_backend() {
    let tool = ChromeTool::new_for_test(Arc::new(FakeChromeManager::default()));
    let err = tool.execute(json!({ "action": "click" }), make_ctx()).await.unwrap_err();
    assert!(matches!(err, ToolError::NotAuthorized(_)));
}
```

Add one ArgusWing test:

```rust
#[test]
fn register_default_tools_includes_chrome() {
    let wing = make_test_wing();
    wing.register_default_tools();
    assert!(wing.tool_manager().get("chrome").is_some());
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool chrome_tool_definition_lists_only_readonly_actions chrome_tool_rejects_denied_action_before_backend
cargo test -p argus-wing register_default_tools_includes_chrome
```

Expected:
- FAIL because `ChromeTool` is not implemented or registered.

**Step 3: Write minimal implementation**

Implement `ChromeTool` as the only public entry point:

```rust
pub struct ChromeTool {
    manager: Arc<ChromeManager>,
    policy: ExplorePolicy,
}
```

Inside `execute()`:
- deserialize strict args
- validate action against `ExplorePolicy`
- dispatch to manager
- map `ChromeToolError` into `ToolError`

Register the tool in `ArgusWing::register_default_tools()`:

```rust
self.tool_manager.register(Arc::new(ChromeTool::new()));
```

Mark the risk level as `RiskLevel::High` or `RiskLevel::Critical` and keep that choice consistent with the approval model.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool chrome_tool_definition_lists_only_readonly_actions chrome_tool_rejects_denied_action_before_backend
cargo test -p argus-wing register_default_tools_includes_chrome
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add crates/argus-tool/src/chrome/tool.rs crates/argus-tool/src/chrome/mod.rs crates/argus-tool/src/lib.rs crates/argus-wing/src/lib.rs
git commit -m "feat: expose chrome tool"
```

### Task 5: Bootstrap real driver install flow and screenshot path control

**Files:**
- Modify: `crates/argus-tool/src/chrome/installer.rs`
- Modify: `crates/argus-tool/src/chrome/manager.rs`
- Modify: `crates/argus-tool/src/chrome/session.rs`
- Modify: `crates/argus-tool/src/chrome/error.rs`

**Step 1: Write the failing tests**

Use test doubles for downloads and process launch:

```rust
#[tokio::test]
async fn installer_writes_into_managed_directories() {
    let paths = ChromePaths::from_home(Path::new("/tmp/home"));
    let downloader = FakeDownloader::with_zip_bytes(fake_driver_zip());
    let installer = ChromeInstaller::new(paths.clone(), downloader);

    let install = installer.ensure_driver("124").await.unwrap();

    assert!(install.original_driver.starts_with(&paths.driver));
    assert!(install.patched_driver.starts_with(&paths.patched));
}

#[tokio::test]
async fn screenshot_rejects_arbitrary_output_path() {
    let manager = make_fake_manager();
    let err = manager
        .screenshot("session-1", Some("../../escape.png"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not allowed"));
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool installer_writes_into_managed_directories screenshot_rejects_arbitrary_output_path
```

Expected:
- FAIL because the installer and screenshot guards are incomplete.

**Step 3: Write minimal implementation**

Implement the first real installer path:
- detect Chrome major version
- download matching driver archive with `reqwest`
- unzip into `driver/`
- patch bytes into `patched/`
- serialize install work with a lock
- produce managed screenshot filenames under `screenshots/`

Keep all system-touching code behind replaceable helpers:

```rust
#[async_trait]
pub trait DriverDownloader: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, ChromeToolError>;
}
```

```rust
pub async fn ensure_driver(&self, chrome_major: &str) -> Result<InstalledDriver, ChromeToolError> {
    // lock -> download if needed -> unzip -> patch -> chmod -> return paths
}
```

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool installer_writes_into_managed_directories screenshot_rejects_arbitrary_output_path
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add crates/argus-tool/src/chrome/installer.rs crates/argus-tool/src/chrome/manager.rs crates/argus-tool/src/chrome/session.rs crates/argus-tool/src/chrome/error.rs
git commit -m "feat: add managed chrome install flow"
```

### Task 6: Add builtin explore-agent template and opt-in smoke coverage

**Files:**
- Create: `agents/chrome_explore.toml`
- Modify: `crates/argus-template/src/manager.rs`
- Create: `crates/argus-tool/tests/chrome_smoke.rs`

**Step 1: Write the failing tests**

Lock down template seeding and opt-in smoke gating:

```rust
#[tokio::test]
async fn seed_builtin_agents_includes_chrome_explore() {
    let manager = make_template_manager_for_test().await;
    manager.seed_builtin_agents().await.unwrap();
    let record = manager.find_by_display_name("Chrome Explore").await.unwrap().unwrap();
    assert_eq!(record.tool_names, vec!["chrome"]);
}
```

```rust
#[tokio::test]
async fn smoke_test_skips_without_env_flag() {
    if std::env::var("ARGUS_CHROME_SMOKE").is_err() {
        return;
    }
    panic!("the skip guard should return before this line");
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-template seed_builtin_agents_includes_chrome_explore
cargo test -p argus-tool --test chrome_smoke smoke_test_skips_without_env_flag
```

Expected:
- FAIL because the new agent definition and smoke test harness do not exist yet.

**Step 3: Write minimal implementation**

Create `agents/chrome_explore.toml` with only the Chrome tool exposed:

```toml
display_name = "Chrome Explore"
description = "Read-only browser exploration agent"
version = "0.1.0"
system_prompt = "You are a read-only browser exploration agent. Use the chrome tool to open pages, wait, extract text, list links, summarize the DOM, and take screenshots. Never attempt form submission, clicking, typing, or script execution."
tool_names = ["chrome"]
```

Create `crates/argus-tool/tests/chrome_smoke.rs`:
- return early unless `ARGUS_CHROME_SMOKE=1`
- return early unless local Chrome is present
- perform one read-only navigation and text extraction through the real manager/tool stack

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-template seed_builtin_agents_includes_chrome_explore
cargo test -p argus-tool --test chrome_smoke smoke_test_skips_without_env_flag
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add agents/chrome_explore.toml crates/argus-template/src/manager.rs crates/argus-tool/tests/chrome_smoke.rs
git commit -m "feat: add chrome explore agent template"
```

### Task 7: Full verification and cleanup

**Files:**
- Modify: any files touched in Tasks 1-6 as needed
- Optional: `crates/argus-tool/CLAUDE.md` if the module list needs updating after the feature lands

**Step 1: Run targeted crate tests**

Run:

```bash
cargo test -p argus-tool
cargo test -p argus-wing
cargo test -p argus-template
```

Expected:
- PASS

**Step 2: Run formatting and lint gates**

Run:

```bash
prek
```

Expected:
- PASS, or autofix formatting and rerun until clean

**Step 3: Run optional smoke test if local Chrome is available**

Run:

```bash
ARGUS_CHROME_SMOKE=1 cargo test -p argus-tool --test chrome_smoke -- --nocapture
```

Expected:
- PASS on machines with local Chrome installed and outbound network access
- Otherwise skip this step and record why

**Step 4: Review risk points**

Manually verify:
- no production `unwrap()` / `expect()` in `crates/argus-tool/src/chrome`
- no caller-controlled install path or screenshot escape path
- denied actions fail before backend execution
- the `Chrome Explore` agent exposes only `chrome`

**Step 5: Commit final polish**

```bash
git add crates/argus-tool crates/argus-wing crates/argus-template agents
git commit -m "test: finalize chrome explore tool rollout"
```
