# Knowledge Provider Abstraction Design

**Date:** 2026-04-02

**Goal:** Refactor the `knowledge` tool so all `gh`, `git`, and GitHub REST interactions are hidden behind provider traits, while the default implementation continues to use the current GitHub-based behavior.

## Summary

Today the `knowledge` tool already abstracts a small part of its GitHub integration through `GitHubTransport`, and its PR write path is partially abstracted through `GitPrExecutor` plus `CliRunner`. That is enough for tests, but it still leaves the business layer coupled to GitHub REST endpoints, `gh` command semantics, and CLI-oriented workspace preparation details.

This design introduces operation-level traits for repository reads and PR writes. The upper knowledge domain keeps its current models, JSON contract, indexing logic, manifest merge logic, and default runtime behavior. Only the external side effects are moved behind interfaces so a future internal company implementation can plug in without rewriting the knowledge domain.

## Goals

- Hide GitHub REST requests behind a repository-read trait.
- Hide `git` and `gh` command workflows behind a PR-write trait.
- Preserve the current `knowledge` tool JSON input and output behavior.
- Preserve the current default implementation path so existing users see no behavior change.
- Keep the integration surface small so a company-specific backend can be added later with minimal changes.

## Non-Goals

- Renaming `GitHubSnapshot`, `GitHubTree`, `GitHubBlob`, or other existing domain types.
- Changing `KnowledgeToolArgs`, `KnowledgeCreatePrArgs`, or the tool schema.
- Reworking manifest parsing, indexing, or PR content generation logic.
- Replacing generics with a fully dynamic plugin architecture.

## Current State

The read path is:

- `KnowledgeTool`
- `DefaultKnowledgeRuntime`
- `KnowledgeRuntimeBackend`
- `GitHubKnowledgeBackend`
- `GitHubKnowledgeClient`
- `GitHubTransport`

The write path is:

- `KnowledgeTool`
- `DefaultKnowledgeRuntime`
- `KnowledgePrRuntime`
- `KnowledgePrService`
- `GitPrExecutor`
- `CliRunner`

This layering helps with unit tests, but the semantic abstraction is still too low-level:

- the read path still thinks in GitHub REST URLs and GitHub-specific transport details
- the write path still exposes a CLI execution mental model rather than a domain-level PR operation model
- swapping to a company-internal Git provider would require either imitating GitHub endpoints or reshaping the upper layers later

## Proposed Architecture

### 1. Add operation-level traits

Introduce two new traits in the knowledge module:

```rust
#[async_trait]
pub trait KnowledgeRepoReadOps: Send + Sync {
    async fn resolve_snapshot(
        &self,
        repo: &KnowledgeRepoDescriptor,
        ref_name: &str,
    ) -> Result<GitHubSnapshot, KnowledgeToolError>;

    async fn read_tree(
        &self,
        repo: &KnowledgeRepoDescriptor,
        rev: &str,
    ) -> Result<GitHubTree, KnowledgeToolError>;

    async fn read_blob(
        &self,
        repo: &KnowledgeRepoDescriptor,
        blob_sha: &str,
    ) -> Result<GitHubBlob, KnowledgeToolError>;
}

#[async_trait]
pub trait KnowledgePrOps: Send + Sync {
    async fn ensure_ready(&self) -> Result<(), KnowledgeToolError>;

    async fn prepare_workspace(
        &self,
        args: &KnowledgeCreatePrArgs,
    ) -> Result<KnowledgePrWorkspace, KnowledgeToolError>;

    async fn commit_and_push(
        &self,
        workspace: &mut KnowledgePrWorkspace,
        commit_message: &str,
    ) -> Result<String, KnowledgeToolError>;

    async fn create_or_reuse_pr(
        &self,
        workspace: &KnowledgePrWorkspace,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<GitPrOutcome, KnowledgeToolError>;
}
```

These traits define provider semantics, not transport mechanics.

### 2. Keep current domain models

The current knowledge domain stays GitHub-shaped for now:

- `GitHubSnapshot`
- `GitHubTree`
- `GitHubBlob`
- `KnowledgePrWorkspace`
- `GitPrOutcome`

This intentionally limits the refactor scope. The company-specific implementation can adapt its own backend into these models first. If that mapping later becomes awkward, a second refactor can generalize the models with lower risk and better real-world feedback.

### 3. Move GitHub and CLI behavior into default adapters

Add two default adapters:

- `GitHubRestKnowledgeRepoOps<T: GitHubTransport>`
- `CliKnowledgePrOps<R: CliRunner>`

Responsibilities:

- `GitHubRestKnowledgeRepoOps` owns GitHub URL construction, REST response parsing, and transport usage
- `CliKnowledgePrOps` owns `gh auth status`, `git clone`, branch detection, commit/push, and PR creation

The existing helper abstractions remain useful:

- `GitHubTransport` stays as the low-level transport seam used by the default read adapter
- `ReqwestGitHubTransport` remains the production transport
- `GitHubKnowledgeClient` remains reusable as an internal helper for the default read adapter
- `CliRunner` remains the low-level command runner used by the default write adapter

### 4. Rewire upper layers to depend on operation traits

Refactor upper layers as follows:

- `GitHubKnowledgeBackend<O: KnowledgeRepoReadOps>` depends on repository-read operations instead of `GitHubTransport`
- `KnowledgePrService<O: KnowledgePrOps>` depends on PR-write operations instead of a GitHub/CLI-specific executor
- `DefaultKnowledgeRuntime` continues to compose a backend plus PR runtime, with defaults that still point to GitHub REST and CLI adapters
- `KnowledgeTool::new()` keeps current behavior

This preserves the current tool wiring while making the provider seam explicit and stable.

## Data Flow

### Read path

```text
KnowledgeTool
  -> DefaultKnowledgeRuntime
  -> GitHubKnowledgeBackend<KnowledgeRepoReadOps>
  -> GitHubRestKnowledgeRepoOps
  -> GitHubKnowledgeClient
  -> GitHubTransport
```

### Write path

```text
KnowledgeTool
  -> DefaultKnowledgeRuntime
  -> KnowledgePrService<KnowledgePrOps>
  -> CliKnowledgePrOps
  -> CliRunner
```

The knowledge domain remains responsible for:

- repo selection
- snapshot caching
- tree exploration and indexing
- manifest discovery and parsing
- manifest patch merge
- file write validation
- PR summary construction

The adapters remain responsible for:

- provider authentication checks
- repository fetch or workspace preparation
- remote reads
- commit and push
- PR creation or reuse

## Compatibility Strategy

This refactor should be incremental rather than a hard replacement.

### Keep existing low-level seams

- Keep `GitHubTransport` and `ReqwestGitHubTransport`
- Keep `GitHubKnowledgeClient`
- Keep `CliRunner`

These stay as implementation details of the default adapters rather than public extension points for future provider work.

### Bridge the current write abstraction

`GitPrExecutor` is already close to the proposed `KnowledgePrOps` shape. The migration can either:

- rename it directly to `KnowledgePrOps`, or
- add `KnowledgePrOps` and bridge the existing executor temporarily

The preferred implementation path is to introduce `KnowledgePrOps` explicitly and then migrate `KnowledgePrService` to the new name, because the new name better communicates that this is the long-term provider seam.

### Preserve public behavior

The following should remain unchanged after the refactor:

- `knowledge` tool definition and schema
- default `KnowledgeTool::new()` behavior
- JSON responses for `list_repos`, `resolve_snapshot`, `explore_tree`, `search_nodes`, `get_node`, `get_content`, `get_neighbors`, and `create_knowledge_pr`

## Error Handling

All provider-specific failures should continue to collapse into `KnowledgeToolError`.

Rules:

- adapters translate transport, CLI, auth, parse, and provider-specific failures into `KnowledgeToolError`
- upper business layers should not encode provider-specific wording beyond the error text they receive
- `ensure_ready()` is intentionally generic so the default CLI adapter can keep checking `gh` plus `git`, while a future internal implementation can validate a different authentication model

This keeps the business layer stable even when providers change.

## Testing Strategy

Testing should move one layer upward so business logic is validated independently from GitHub and CLI details.

### Adapter tests

- keep or expand GitHub REST adapter tests with fake `GitHubTransport`
- keep or expand CLI adapter tests with fake `CliRunner`

### Business-layer tests

- update `GitHubKnowledgeBackend` tests to use fake `KnowledgeRepoReadOps`
- update `KnowledgePrService` tests to use fake `KnowledgePrOps`

These tests should focus on snapshot caching, manifest probing, file validation, merge behavior, and response shaping rather than concrete transport details.

### Tool-level regression tests

Preserve and update the existing integration-style tests:

- `crates/argus-tool/tests/knowledge_flow.rs`
- `crates/argus-tool/tests/knowledge_create_pr.rs`

These guard the external contract so the refactor stays behavior-preserving.

## Migration Notes

The intended near-term outcome is:

- the default implementation remains GitHub-based
- company-internal Git integrations can be added by implementing `KnowledgeRepoReadOps` and `KnowledgePrOps`
- no caller above the knowledge module needs to know whether the backend uses GitHub REST, `gh`, or an internal system

That gives the codebase a cleaner extension seam now without paying the much larger cost of fully renaming the knowledge domain today.
