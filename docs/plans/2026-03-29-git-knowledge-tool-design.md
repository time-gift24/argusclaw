# Git Knowledge Tool Design

## Summary

Build a read-only `knowledge` tool that treats one remote Git repository as one knowledge base.

For v1, the tool supports progressive exploration over GitHub-hosted repositories without cloning them into the local workspace. The `main` branch remains the source of truth, while future write flows can layer branch edits and pull requests on top of the same data model.

## Background

We want a tool that can help an LLM explore a repository as knowledge instead of as a raw filesystem.

Two product constraints shape the design:

- One repository represents one knowledge base.
- The tool must not clone the repository into the local workspace.

This means the system cannot rely on a local checkout plus filesystem tools for exploration. Instead, it must resolve a remote Git snapshot, read tree and blob data on demand, and build a knowledge-oriented view incrementally.

## Goals

- Expose one `knowledge` tool to agents through the existing tool registry.
- Model one Git repository as one knowledge base.
- Support progressive exploration with tree browsing, search, node inspection, local content expansion, and neighbor traversal.
- Keep results tied to a stable Git snapshot for the duration of a search flow.
- Support zero-config discovery from repository conventions.
- Support optional manifest-based metadata overrides.
- Avoid cloning repositories into the local workspace.

## Non-Goals

- Branch creation, commits, or pull request submission in v1.
- Support for arbitrary Git providers in v1.
- Full-repository full-text indexing across all blobs.
- Deep parsing of binary assets such as images, PDFs, or office files.
- Persisting a local mirror of the remote repository.

## Key Constraints

- Provider scope for v1 is GitHub only.
- Exploration defaults to the repository's `main` branch.
- The tool must operate against remote Git data, not a checked-out local repository.
- All content answers must be traceable back to repository path and line span where possible.
- Tool inputs must be strict and bounded to keep LLM behavior predictable.

## High-Level Approach

Expose one `knowledge` tool with a small action-based schema:

1. `resolve_snapshot`
2. `list_repos`
3. `explore_tree`
4. `search_nodes`
5. `get_node`
6. `get_content`
7. `get_neighbors`

Internally, the design separates three layers:

1. `GitProvider`
   Resolves refs, lists trees, and fetches blobs from GitHub.
2. `KnowledgeIndexer`
   Builds file and section nodes incrementally from conventions plus optional manifest overrides.
3. `KnowledgeTool`
   Validates tool arguments, enforces result limits, and exposes the final action API.

This keeps the public interface simple while letting the implementation evolve from basic metadata search to richer graph traversal later.

## Progressive Exploration Model

The LLM does not search raw Git directly. It searches a progressive knowledge view built from Git snapshots.

Typical search loop:

1. Resolve `repo_id + ref` into an immutable snapshot.
2. Explore tree structure to understand the information map.
3. Search nodes by title, path, aliases, tags, summaries, and known headings.
4. Inspect a specific node's metadata and relationships.
5. Expand only the needed content window.
6. Traverse lightweight relationships to continue exploration.

This keeps search cheap and explainable:

- Tree and metadata first
- Partial content second
- Full blob access only when required

## Snapshot Model

Every exploration flow is anchored to a snapshot:

```json
{
  "snapshot_id": "snap_123",
  "repo_id": "acme-docs",
  "ref": "main",
  "rev": "abc123def456"
}
```

All later actions should prefer `snapshot_id` over a mutable branch name. This prevents the LLM from mixing results from different commits while browsing.

## Repository Metadata

The tool should store repository descriptors separately from snapshots:

```json
{
  "repo_id": "acme-docs",
  "provider": "github",
  "remote": {
    "owner": "acme",
    "name": "docs",
    "url": "https://github.com/acme/docs"
  },
  "default_branch": "main",
  "manifest_paths": [
    ".knowledge/repo.json",
    "knowledge.json"
  ]
}
```

`repo_id` is the stable handle the agent uses. Repository descriptors can be static configuration or later come from a repository registry.

## Hybrid Index Model

The index uses a hybrid model:

- `file` nodes mirror the repository's physical structure.
- `section` nodes represent logical knowledge units within files.

This gives us the best of both worlds:

- Physical traceability to Git paths
- Logical exploration closer to page-like knowledge navigation

The index is built progressively:

1. Read repository tree metadata.
2. Build `file` nodes from paths and filenames.
3. Read optional manifest files for metadata overrides.
4. Parse blobs only when a file is explored or searched deeply.
5. Generate `section` nodes from Markdown headings and other supported conventions.

## Manifest Strategy

The manifest is optional. Convention-based discovery is the default.

Manifest responsibilities:

- Override titles and summaries
- Define aliases and tags
- Define stable logical node IDs
- Add lightweight relations
- Exclude or prioritize content

Suggested repository manifest schema:

```json
{
  "$schema": "https://argus.dev/schema/knowledge-repo-v1.json",
  "version": 1,
  "repo": {
    "title": "Acme Docs",
    "default_branch": "main",
    "include": ["README.md", "docs/**/*.md"],
    "exclude": ["archive/**"],
    "entrypoints": ["README.md", "docs/"]
  },
  "files": [
    {
      "path": "README.md",
      "title": "Overview",
      "summary": "Project overview and navigation",
      "tags": ["intro"],
      "aliases": ["home"]
    }
  ],
  "nodes": [
    {
      "id": "auth/refresh-flow",
      "source": {
        "path": "docs/auth.md",
        "heading": "Refresh Flow"
      },
      "title": "Refresh Flow",
      "summary": "How token refresh works",
      "tags": ["auth"],
      "aliases": ["token refresh"],
      "relations": [
        { "type": "related", "target": "auth/login-flow" }
      ]
    }
  ]
}
```

Automatic IDs should default to a deterministic form such as `path#heading-slug`. The manifest may override this with a stable logical ID where needed.

## Core Knowledge Node Schema

All knowledge results should normalize to one public node shape:

```json
{
  "id": "auth/refresh-flow",
  "kind": "file | section",
  "repo_id": "acme-docs",
  "snapshot_id": "snap_123",
  "title": "Refresh Flow",
  "path": "docs/auth.md",
  "anchor": "refresh-flow",
  "summary": "How token refresh works",
  "aliases": ["token refresh"],
  "tags": ["auth"],
  "source": {
    "path": "docs/auth.md",
    "blob_sha": "abc123",
    "start_line": 42,
    "end_line": 88
  },
  "relations": [
    { "type": "contains | related | alias_of", "target": "auth/login-flow" }
  ]
}
```

The important invariant is that every logical node remains traceable back to Git source data.

## Tool Contract

The agent sees one tool named `knowledge`.

Top-level schema:

```json
{
  "action": "resolve_snapshot | list_repos | explore_tree | search_nodes | get_node | get_content | get_neighbors",
  "repo_id": "required except list_repos",
  "snapshot_id": "preferred after resolve_snapshot",
  "ref": "optional for resolve_snapshot, defaults to main",
  "cursor": "optional",
  "limit": 20
}
```

### `resolve_snapshot`

Input:

```json
{
  "action": "resolve_snapshot",
  "repo_id": "acme-docs",
  "ref": "main"
}
```

Output:

```json
{
  "snapshot_id": "snap_123",
  "repo_id": "acme-docs",
  "ref": "main",
  "rev": "abc123def456"
}
```

### `explore_tree`

Input:

```json
{
  "action": "explore_tree",
  "snapshot_id": "snap_123",
  "path": "/docs",
  "depth": 2,
  "include_summaries": true
}
```

Output returns tree entries only, never full content:

```json
{
  "path": "/docs",
  "entries": [
    {
      "kind": "dir | file | section_group",
      "title": "Auth",
      "path": "/docs/auth.md",
      "child_count": 3,
      "summary_hint": "Authentication and session topics"
    }
  ],
  "truncated": false
}
```

### `search_nodes`

Input:

```json
{
  "action": "search_nodes",
  "snapshot_id": "snap_123",
  "query": "token refresh",
  "scope_path": "/docs/auth",
  "kinds": ["file", "section"],
  "limit": 8
}
```

Output returns compact candidates:

```json
{
  "results": [
    {
      "node_id": "auth/refresh-flow",
      "title": "Refresh Flow",
      "path": "docs/auth.md",
      "anchor": "refresh-flow",
      "summary": "How token refresh works",
      "match_reasons": ["title", "alias"],
      "score": 0.91
    }
  ],
  "truncated": false
}
```

### `get_node`

Input:

```json
{
  "action": "get_node",
  "snapshot_id": "snap_123",
  "node_id": "auth/refresh-flow"
}
```

Output returns metadata and relationships, not large content blocks.

### `get_content`

Input:

```json
{
  "action": "get_content",
  "snapshot_id": "snap_123",
  "node_id": "auth/refresh-flow",
  "max_chars": 2400
}
```

Output returns bounded content with traceable source metadata:

```json
{
  "content": "...",
  "truncated": true,
  "next_cursor": "cursor_2",
  "source": {
    "path": "docs/auth.md",
    "start_line": 42,
    "end_line": 88
  }
}
```

### `get_neighbors`

Input:

```json
{
  "action": "get_neighbors",
  "snapshot_id": "snap_123",
  "node_id": "auth/refresh-flow",
  "relation_types": ["contains", "related", "alias_of"]
}
```

Output returns lightweight graph traversal data.

## Search Guarantees

For v1, `search_nodes` is a progressive structured search, not a full blob-level full-text index.

It should reliably search:

- File paths and file names
- Manifest titles, summaries, aliases, and tags
- Known section headings
- Already-expanded node metadata

It may optionally deepen candidate scoring by fetching a small number of relevant blobs on demand. It should not promise full-repository exhaustive content search across all files.

## Remote Access Strategy

The implementation should talk to GitHub using remote Git-facing APIs and metadata endpoints.

Expected operations:

- Resolve branch name to commit SHA
- List tree entries for a given revision
- Read blob content for selected files
- Read manifest files at known paths

The implementation must not:

- Run `git clone`
- Create a working copy of the repository
- Depend on local filesystem traversal of repository contents

## Caching Strategy

Use lightweight caches, never a local repository mirror.

### Snapshot Cache

Stores:

- `repo_id + ref -> rev`

Purpose:

- Avoid repeated ref resolution during one exploration session

### Node Cache

Stores:

- Parsed manifest data
- Tree listings
- Heading summaries
- Expanded nodes for a specific snapshot

Cache keys must include `snapshot_id` so that a moving branch never contaminates results from another commit.

## Error Handling

The tool should provide explicit, LLM-friendly failure modes:

- `NotFound`
  Repository, path, node, or manifest does not exist
- `RefChanged`
  A stale snapshot or branch resolution mismatch requires refreshing the snapshot
- `RateLimited`
  GitHub API rate limiting requires retry or backoff
- `UnsupportedContent`
  Blob type or size is outside v1 parsing support
- `InvalidArguments`
  Unknown fields or invalid argument combinations are rejected at validation time

## Safety and Bounds

- Reject unknown fields during argument deserialization.
- Bound all list and search result counts.
- Bound content extraction by size and cursor window.
- Reject oversized or unsupported blobs for section parsing.
- Keep provider-specific credentials outside the public tool contract.

## Testing Strategy

The implementation should be verified with unit and integration coverage for:

- Tool schema validation and argument rejection
- Snapshot consistency across multi-step exploration flows
- Tree exploration bounds and truncation behavior
- Lazy parsing of file contents only after explicit need
- Manifest override behavior
- Stable node ID generation
- No-local-clone invariant
- Content provenance mapping back to file path and line span
- Rate-limit and not-found error handling

## Future Extension Path

This v1 design keeps the write path out of scope, but it deliberately preserves the primitives needed for later contribution workflows:

- `snapshot_id` as a stable read baseline
- logical node IDs for review and diff targeting
- source mapping back to Git paths and line ranges

That makes it possible to later add:

- branch-based edits
- generated patches
- pull request creation
- review flows against `main` as the final truth

without redesigning the knowledge model.
