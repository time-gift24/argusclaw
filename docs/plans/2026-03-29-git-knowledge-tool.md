# Git Knowledge Tool Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a read-only `knowledge` tool that progressively explores GitHub-hosted repositories as knowledge bases without cloning them into the local workspace.

**Architecture:** Build a new `crates/argus-tool/src/knowledge/` module with a strict action schema, a repo registry, a fakeable GitHub client, a progressive hybrid indexer, and one exposed `KnowledgeTool`. Register the tool in `argus-wing`, keep all exploration pinned to snapshot IDs, and support zero-config discovery plus optional manifest overrides.

**Tech Stack:** Rust 2024, Tokio, Reqwest 0.12, Serde, Serde JSON, DashMap, Regex, Dirs, Base64, GitHub REST API

---

## Execution Preconditions

- Run implementation from a dedicated `.worktrees/...` worktree, not from the root `main` checkout. Use `@using-git-worktrees`.
- Keep `docs/plans/` changes on `main`; implement Rust code in the worktree.
- Follow `@test-driven-development` discipline for every task: write the failing test, run it, implement the minimum, rerun, then commit.
- Before claiming success on any task, run the listed verification commands and follow `@verification-before-completion`.
- Initialize local checks if needed with `cargo install prek && prek install`.

### Task 1: Scaffold the knowledge module, strict action schema, and repo registry

**Files:**
- Modify: `crates/argus-tool/Cargo.toml`
- Modify: `crates/argus-tool/src/lib.rs`
- Create: `crates/argus-tool/src/knowledge/mod.rs`
- Create: `crates/argus-tool/src/knowledge/models.rs`
- Create: `crates/argus-tool/src/knowledge/error.rs`
- Create: `crates/argus-tool/src/knowledge/registry.rs`

**Step 1: Write the failing tests**

Add unit tests that lock down the schema and registry invariants:

```rust
#[test]
fn knowledge_scaffold_rejects_unknown_fields() {
    let err = KnowledgeToolArgs::parse(serde_json::json!({
        "action": "search_nodes",
        "repo_id": "acme-docs",
        "query": "refresh",
        "unexpected": true
    }))
    .unwrap_err();

    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn knowledge_scaffold_resolve_snapshot_requires_repo_id() {
    let err = KnowledgeToolArgs::parse(serde_json::json!({
        "action": "resolve_snapshot"
    }))
    .unwrap_err();

    assert!(err.to_string().contains("repo_id"));
}

#[test]
fn knowledge_scaffold_registry_default_path_uses_arguswing_home() {
    let path = KnowledgeRepoRegistry::default_path_from_home(std::path::Path::new("/tmp/home"));
    assert_eq!(
        path,
        std::path::PathBuf::from("/tmp/home/.arguswing/knowledge/repos.json")
    );
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool knowledge_scaffold_
```

Expected:
- FAIL because the `knowledge` module, args parser, and registry types do not exist yet.

**Step 3: Write minimal implementation**

Add `dirs` to `crates/argus-tool/Cargo.toml`, then implement the minimal scaffold:

```toml
# crates/argus-tool/Cargo.toml
dirs = { workspace = true }
```

```rust
// crates/argus-tool/src/knowledge/models.rs
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeToolArgs {
    pub action: KnowledgeAction,
    #[serde(default)]
    pub repo_id: Option<String>,
    #[serde(default)]
    pub snapshot_id: Option<String>,
    #[serde(default)]
    pub r#ref: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}
```

```rust
// crates/argus-tool/src/knowledge/registry.rs
pub struct KnowledgeRepoRegistry;

impl KnowledgeRepoRegistry {
    pub fn default_path_from_home(home: &Path) -> PathBuf {
        home.join(".arguswing").join("knowledge").join("repos.json")
    }
}
```

Implement:

- `KnowledgeAction`
- `KnowledgeToolArgs::parse(value)`
- validation rules for required `repo_id` or `snapshot_id`
- `KnowledgeRepoDescriptor`
- `KnowledgeToolError`
- `pub mod knowledge;` in `crates/argus-tool/src/lib.rs`

Do not add network code yet.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool knowledge_scaffold_
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add crates/argus-tool/Cargo.toml crates/argus-tool/src/lib.rs crates/argus-tool/src/knowledge
git commit -m "feat: scaffold knowledge tool models"
```

### Task 2: Add a fakeable GitHub client for snapshots, trees, and blobs

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/argus-tool/Cargo.toml`
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Modify: `crates/argus-tool/src/knowledge/models.rs`
- Modify: `crates/argus-tool/src/knowledge/error.rs`
- Create: `crates/argus-tool/src/knowledge/github.rs`

**Step 1: Write the failing tests**

Add unit tests around a fake transport so the GitHub logic stays offline and deterministic:

```rust
#[tokio::test]
async fn knowledge_github_resolve_snapshot_parses_head_commit() {
    let client = GitHubKnowledgeClient::new_for_test(FakeGitHubTransport::with_json(vec![
        serde_json::json!({ "object": { "sha": "abc123" } })
    ]));

    let snapshot = client.resolve_snapshot("acme", "docs", "main").await.unwrap();
    assert_eq!(snapshot.rev, "abc123");
}

#[tokio::test]
async fn knowledge_github_read_tree_maps_entries() {
    let client = GitHubKnowledgeClient::new_for_test(FakeGitHubTransport::with_json(vec![
        serde_json::json!({
            "tree": [
                { "path": "README.md", "type": "blob", "sha": "blob-1" },
                { "path": "docs", "type": "tree", "sha": "tree-1" }
            ]
        })
    ]));

    let tree = client.read_tree("acme", "docs", "abc123").await.unwrap();
    assert_eq!(tree.entries.len(), 2);
}

#[tokio::test]
async fn knowledge_github_read_blob_decodes_base64() {
    let client = GitHubKnowledgeClient::new_for_test(FakeGitHubTransport::with_json(vec![
        serde_json::json!({
            "sha": "blob-1",
            "content": "IyBUaXRsZQoKQm9keQ==",
            "encoding": "base64"
        })
    ]));

    let blob = client.read_blob("acme", "docs", "blob-1").await.unwrap();
    assert!(blob.text.contains("# Title"));
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool knowledge_github_
```

Expected:
- FAIL because the GitHub client and transport abstraction do not exist yet.

**Step 3: Write minimal implementation**

Add `base64` to workspace and crate dependencies:

```toml
# Cargo.toml
base64 = "0.22"
```

```toml
# crates/argus-tool/Cargo.toml
base64 = { workspace = true }
```

Implement a fakeable client:

```rust
#[async_trait]
pub trait GitHubTransport: Send + Sync {
    async fn get_json(&self, url: &str) -> Result<serde_json::Value, KnowledgeToolError>;
}

pub struct GitHubKnowledgeClient<T: GitHubTransport> {
    transport: T,
}
```

Implement:

- `resolve_snapshot(owner, repo, ref_name)`
- `read_tree(owner, repo, rev)`
- `read_blob(owner, repo, blob_sha)`
- GitHub response DTOs
- base64 decoding for blob content
- explicit rate-limit and not-found mapping

Do not parse manifests or Markdown headings yet.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool knowledge_github_
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add Cargo.toml crates/argus-tool/Cargo.toml crates/argus-tool/src/knowledge
git commit -m "feat: add github knowledge client"
```

### Task 3: Parse manifests and Markdown sections into stable knowledge nodes

**Files:**
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Modify: `crates/argus-tool/src/knowledge/models.rs`
- Modify: `crates/argus-tool/src/knowledge/error.rs`
- Create: `crates/argus-tool/src/knowledge/manifest.rs`
- Create: `crates/argus-tool/src/knowledge/markdown.rs`

**Step 1: Write the failing tests**

Add pure unit tests for manifest override behavior and section parsing:

```rust
#[test]
fn knowledge_manifest_overrides_file_title_and_aliases() {
    let manifest = RepositoryManifest::from_json(serde_json::json!({
        "version": 1,
        "files": [
            {
                "path": "README.md",
                "title": "Overview",
                "aliases": ["home"]
            }
        ]
    }))
    .unwrap();

    let meta = manifest.file_override("README.md").unwrap();
    assert_eq!(meta.title.as_deref(), Some("Overview"));
    assert_eq!(meta.aliases, vec!["home"]);
}

#[test]
fn knowledge_manifest_extracts_markdown_sections_with_line_spans() {
    let sections = parse_markdown_sections(
        "docs/auth.md",
        "# Auth\nintro\n## Refresh Flow\nbody\n## Login Flow\nbody\n",
    );

    assert_eq!(sections[0].anchor, "auth");
    assert_eq!(sections[1].start_line, 3);
    assert_eq!(sections[1].end_line, 4);
}

#[test]
fn knowledge_manifest_declared_id_wins_over_generated_id() {
    let manifest = RepositoryManifest::from_json(serde_json::json!({
        "version": 1,
        "nodes": [
            {
                "id": "auth/refresh-flow",
                "source": { "path": "docs/auth.md", "heading": "Refresh Flow" }
            }
        ]
    }))
    .unwrap();

    let node_id = manifest
        .resolve_section_id("docs/auth.md", "Refresh Flow", "docs/auth.md#refresh-flow");
    assert_eq!(node_id, "auth/refresh-flow");
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool knowledge_manifest_
```

Expected:
- FAIL because manifest and Markdown parsing helpers do not exist yet.

**Step 3: Write minimal implementation**

Implement:

```rust
pub struct RepositoryManifest {
    pub version: u32,
    pub repo: Option<RepositoryManifestMeta>,
    pub files: Vec<FileOverride>,
    pub nodes: Vec<NodeOverride>,
}

pub fn parse_markdown_sections(path: &str, content: &str) -> Vec<ParsedSection> {
    // scan lines, detect #..###### headings, compute anchors and line spans
}
```

Include:

- fixed manifest paths: `.knowledge/repo.json`, `knowledge.json`
- manifest lookup helpers by file path and heading
- deterministic generated section IDs in the form `path#slug`
- line-by-line Markdown heading parsing without adding a heavyweight parser

Do not build search or caching yet.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool knowledge_manifest_
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add crates/argus-tool/src/knowledge
git commit -m "feat: parse knowledge manifests and markdown sections"
```

### Task 4: Build the snapshot-scoped hybrid index and progressive search service

**Files:**
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Modify: `crates/argus-tool/src/knowledge/models.rs`
- Create: `crates/argus-tool/src/knowledge/cache.rs`
- Create: `crates/argus-tool/src/knowledge/indexer.rs`

**Step 1: Write the failing tests**

Add unit tests that lock down lazy indexing and bounded exploration:

```rust
#[tokio::test]
async fn knowledge_index_explore_tree_lists_entries_without_blob_fetch() {
    let backend = RecordingKnowledgeBackend::tree_only();
    let indexer = KnowledgeIndexer::new(backend.clone());

    let tree = indexer.explore_tree("snap-1", "/docs", 1).await.unwrap();
    assert_eq!(tree.entries.len(), 2);
    assert_eq!(backend.blob_fetch_count(), 0);
}

#[tokio::test]
async fn knowledge_index_search_matches_manifest_and_heading_metadata() {
    let backend = RecordingKnowledgeBackend::with_auth_docs();
    let indexer = KnowledgeIndexer::new(backend);

    let results = indexer
        .search_nodes("snap-1", "token refresh", Some("/docs/auth"), 8)
        .await
        .unwrap();

    assert!(results.iter().any(|node| node.title == "Refresh Flow"));
}

#[tokio::test]
async fn knowledge_index_get_content_is_bounded_and_cursorized() {
    let backend = RecordingKnowledgeBackend::with_large_section();
    let indexer = KnowledgeIndexer::new(backend);

    let page = indexer
        .get_content("snap-1", "docs/auth.md#refresh-flow", Some(120))
        .await
        .unwrap();

    assert!(page.truncated);
    assert!(page.next_cursor.is_some());
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool knowledge_index_
```

Expected:
- FAIL because the cache and indexer services do not exist yet.

**Step 3: Write minimal implementation**

Implement the progressive service:

```rust
pub struct KnowledgeIndexer<B> {
    backend: Arc<B>,
    snapshots: DashMap<String, SnapshotCache>,
}

pub struct SnapshotCache {
    pub tree: Option<RemoteTree>,
    pub manifest: Option<RepositoryManifest>,
    pub nodes: DashMap<String, KnowledgeNode>,
}
```

Implement:

- file-node creation from remote tree entries
- lazy section expansion only when a file is inspected or searched deeply
- manifest override merge into generated nodes
- simple metadata search over path, title, aliases, tags, summaries, and headings
- `get_node`, `get_content`, and `get_neighbors`
- cursorized content windows with explicit truncation

Keep search deliberately narrow: do not add full-repository full-text indexing.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool knowledge_index_
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add crates/argus-tool/src/knowledge
git commit -m "feat: add progressive knowledge indexer"
```

### Task 5: Expose the `knowledge` tool and register it in ArgusWing

**Files:**
- Modify: `crates/argus-tool/src/lib.rs`
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Create: `crates/argus-tool/src/knowledge/tool.rs`
- Modify: `crates/argus-wing/src/lib.rs`

**Step 1: Write the failing tests**

Add API-facing tests that lock down the external contract:

```rust
#[test]
fn knowledge_tool_definition_lists_expected_actions() {
    let tool = KnowledgeTool::new_for_test(FakeKnowledgeRuntime::default());
    let def = tool.definition();

    assert_eq!(def.name, "knowledge");
    assert!(def.description.contains("GitHub"));
    assert!(def.parameters.to_string().contains("resolve_snapshot"));
    assert!(def.parameters.to_string().contains("search_nodes"));
}

#[tokio::test]
async fn knowledge_tool_rejects_invalid_action_before_runtime() {
    let tool = KnowledgeTool::new_for_test(FakeKnowledgeRuntime::default());
    let err = tool
        .execute(
            serde_json::json!({ "action": "unknown_action" }),
            make_ctx(),
        )
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unknown variant"));
}

#[test]
fn register_default_tools_includes_knowledge() {
    let wing = ArgusWing::new_for_test();
    wing.register_default_tools();
    assert!(wing.tool_manager().get("knowledge").is_some());
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool knowledge_tool_
cargo test -p argus-wing register_default_tools_includes_knowledge
```

Expected:
- FAIL because `KnowledgeTool` is not exposed or registered yet.

**Step 3: Write minimal implementation**

Implement the public tool:

```rust
pub struct KnowledgeTool<R = DefaultKnowledgeRuntime> {
    runtime: Arc<R>,
}

#[async_trait]
impl<R: KnowledgeRuntime> NamedTool for KnowledgeTool<R> {
    fn name(&self) -> &str { "knowledge" }
    fn risk_level(&self) -> RiskLevel { RiskLevel::Medium }
    async fn execute(&self, input: serde_json::Value, _ctx: Arc<ToolExecutionContext>) -> Result<serde_json::Value, ToolError> {
        // parse args, dispatch to runtime, serialize bounded result
    }
}
```

Also:

- re-export `KnowledgeTool` from `crates/argus-tool/src/lib.rs`
- register `KnowledgeTool::new()` in `ArgusWing::register_default_tools()`
- keep one public tool name with action dispatch inside

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool knowledge_tool_
cargo test -p argus-wing register_default_tools_includes_knowledge
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add crates/argus-tool/src/lib.rs crates/argus-tool/src/knowledge crates/argus-wing/src/lib.rs
git commit -m "feat: expose knowledge tool"
```

### Task 6: Add end-to-end progressive flow coverage and guard the no-local-clone invariant

**Files:**
- Create: `crates/argus-tool/tests/knowledge_flow.rs`
- Modify: `crates/argus-tool/src/knowledge/github.rs`
- Modify: `crates/argus-tool/src/knowledge/indexer.rs`
- Modify: `crates/argus-tool/src/knowledge/tool.rs`

**Step 1: Write the failing tests**

Add integration coverage around the full read flow:

```rust
#[tokio::test]
async fn knowledge_flow_progressive_read_path_works_end_to_end() {
    let tool = make_fake_knowledge_tool();

    let snapshot = tool
        .execute(json!({ "action": "resolve_snapshot", "repo_id": "acme-docs" }), make_ctx())
        .await
        .unwrap();
    let snapshot_id = snapshot["snapshot_id"].as_str().unwrap();

    let tree = tool
        .execute(json!({ "action": "explore_tree", "snapshot_id": snapshot_id, "path": "/", "depth": 2 }), make_ctx())
        .await
        .unwrap();
    assert!(tree["entries"].as_array().unwrap().len() > 0);

    let hits = tool
        .execute(json!({ "action": "search_nodes", "snapshot_id": snapshot_id, "query": "refresh flow" }), make_ctx())
        .await
        .unwrap();
    assert!(hits["results"].as_array().unwrap().len() > 0);
}

#[tokio::test]
async fn knowledge_flow_missing_manifest_falls_back_to_convention() {
    let tool = make_fake_knowledge_tool_without_manifest();
    let result = tool
        .execute(json!({ "action": "search_nodes", "snapshot_id": "snap-1", "query": "Auth" }), make_ctx())
        .await
        .unwrap();

    assert!(result["results"].as_array().unwrap().len() > 0);
}

#[tokio::test]
async fn knowledge_flow_public_api_never_exposes_local_repo_path() {
    let tool = make_fake_knowledge_tool();
    let result = tool
        .execute(json!({ "action": "get_node", "snapshot_id": "snap-1", "node_id": "auth/refresh-flow" }), make_ctx())
        .await
        .unwrap();

    assert!(result.to_string().contains("\"path\":\"docs/auth.md\""));
    assert!(!result.to_string().contains("/Users/"));
    assert!(!result.to_string().contains(".worktrees"));
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-tool --test knowledge_flow
```

Expected:
- FAIL because the integration helpers and progressive flow are not fully wired together yet.

**Step 3: Write minimal implementation**

Finish the remaining seams:

- add `new_for_test` constructors for the runtime and tool
- ensure all integration paths operate from remote descriptors and snapshot IDs only
- ensure manifest loading is optional
- ensure outputs expose repository-relative paths only
- verify no code path shells out to `git clone` or expects a local checkout path

If a helper currently depends on a local path, remove that dependency instead of papering over it in tests.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-tool --test knowledge_flow
cargo test -p argus-tool knowledge_
cargo test -p argus-wing register_default_tools_includes_knowledge
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add crates/argus-tool/tests/knowledge_flow.rs crates/argus-tool/src/knowledge crates/argus-wing/src/lib.rs
git commit -m "test: cover knowledge progressive flow"
```

## Final Verification

After Task 6, run the full verification set from the worktree:

```bash
cargo test -p argus-tool knowledge_
cargo test -p argus-tool --test knowledge_flow
cargo test -p argus-wing register_default_tools_includes_knowledge
prek
```

Expected:
- All tests PASS
- `prek` passes or auto-fixes formatting issues without new failures

If `prek` changes files, restage them and create one final commit instead of leaving the worktree dirty.
