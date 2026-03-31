# Knowledge PR Capability Design

## Context

The current `knowledge` tool is read-only. It can list repositories, resolve snapshots, explore trees, search nodes, and read content, but it cannot materialize knowledge updates back into a repository or open a pull request. The requirement is to add a first-class `knowledge` capability that can take LLM-prepared knowledge changes, update documentation plus `.knowledge/repo.json` or `knowledge.json`, and open a GitHub pull request using the local `git` and `gh` CLIs.

## Goals

- Add a `knowledge.create_knowledge_pr` action to the existing `knowledge` tool.
- Allow the action to target any GitHub repository, even if it is not yet registered as a knowledge repo.
- Write documentation files and create or update `.knowledge/repo.json` or `knowledge.json` in the same change set.
- Use local `git` and `gh` CLIs for branch creation, push, and PR creation.
- Require approval only for the dangerous write action, not for existing read-only knowledge actions.
- Return structured PR metadata that the outer LLM can reference in follow-up responses.

## Non-Goals

- Generating knowledge content inside the tool.
- Crawling external websites or collecting new evidence inside the tool.
- Supporting non-GitHub providers in `v1`.
- Supporting batch PR creation across multiple repositories in one call.
- Turning `knowledge` into a general-purpose git automation tool.

## Chosen Approach

Use a payload-driven write action inside `knowledge`. The outer LLM remains responsible for deciding what documentation and manifest changes should be made. The tool is responsible for validating those changes, applying them in an isolated temporary checkout, merging the manifest, and opening the PR.

This keeps the architecture aligned with the existing tool model: the LLM plans, the tool executes. It also avoids embedding a second “hidden agent” inside the tool.

## Tool Contract

Add a new action named `create_knowledge_pr`.

The action accepts:

- `target_repo`: GitHub repository in `owner/name` form.
- `base_ref`: Optional base branch, defaulting to `main`.
- `branch`: Optional explicit branch name.
- `pr_title`: Required PR title.
- `pr_body`: Required PR body.
- `draft`: Optional draft PR flag.
- `files`: Required list of file writes, each with `path` and `content`.
- `manifest`: Required or optional manifest patch payload containing:
  - `path`: optional explicit manifest path
  - `repo`: optional manifest repo metadata patch
  - `files`: optional manifest file entries
  - `nodes`: optional manifest node entries

The action returns:

- `target_repo`
- `base_ref`
- `branch`
- `commit_sha`
- `pr_url`
- `manifest_path`
- `changed_files`
- `created_files`
- `updated_files`
- `summary`

## Execution Flow

1. Validate arguments and repository-relative file paths.
2. Verify GitHub authentication with `gh auth status`.
3. Create a temporary working directory.
4. Clone the target repository with local `git` or `gh`.
5. Checkout the requested base branch and create the working branch.
6. Write requested documentation files into the checkout.
7. Detect an existing manifest path in this order:
   - explicit `manifest.path`
   - `.knowledge/repo.json`
   - `knowledge.json`
   - default `.knowledge/repo.json`
8. Read and parse the existing manifest if present.
9. Merge the incoming manifest patch:
   - overwrite explicitly provided repo metadata fields
   - upsert `files` by `path`
   - upsert `nodes` by `id`
   - deduplicate `include`, `exclude`, and `entrypoints`
10. Serialize the manifest with stable formatting.
11. Run `git add`, `git commit`, `git push -u`.
12. Use `gh pr create` to open the PR.
13. Return structured metadata for the caller.

## Approval Model

The existing approval system gates by tool name, not by action. Making the whole `knowledge` tool dangerous would degrade read-only browsing flows.

To avoid that, approval will become action-aware for this tool:

- Keep `knowledge` available for read-only operations without new approval prompts.
- In the approval hook, derive an action-scoped approval key when `tool_name == "knowledge"`.
- For `action = "create_knowledge_pr"`, derive `knowledge_create_knowledge_pr`.
- Check policy against both:
  - the action-scoped key first
  - the raw tool name as fallback

`ApprovalPolicy::default()` should include `knowledge_create_knowledge_pr` alongside `shell` and `http`, so PR creation is gated by default.

## Reliability and Safety

- Reject absolute paths, `..`, and `.git/**`.
- Reject malformed manifest payloads before any write occurs.
- Use explicit argument arrays with `tokio::process::Command`; do not shell out via `sh -c`.
- Keep all repository mutations inside a temporary checkout.
- If failure happens before push, return an error and clean the temp directory.
- If push succeeds but PR creation fails, return the remote branch name and recovery guidance.
- If an existing PR or branch is detected, prefer reusing it instead of blindly creating duplicates.

## Testing Strategy

Cover the feature at three layers:

### 1. Tool contract tests

- `KnowledgeAction` includes `create_knowledge_pr`.
- Tool definition exposes the new action and parameters.
- Argument parsing rejects unknown fields and malformed payloads.

### 2. Pure logic tests

- Path validation rejects unsafe writes.
- Manifest merge creates a new manifest when absent.
- Manifest merge upserts `files` by `path`.
- Manifest merge upserts `nodes` by `id`.
- Manifest merge deduplicates repo arrays.

### 3. Execution and approval tests

- Fake executor tests for clone -> branch -> write -> commit -> push -> PR success.
- Failure tests for auth failure, missing repo, push failure, PR creation failure, and existing PR reuse.
- Approval hook tests for `knowledge_create_knowledge_pr` gating without affecting read actions.

## Proposed File Layout

- Modify `crates/argus-tool/src/knowledge/models.rs`
- Modify `crates/argus-tool/src/knowledge/tool.rs`
- Modify `crates/argus-tool/src/knowledge/mod.rs`
- Create `crates/argus-tool/src/knowledge/pr.rs`
- Modify `crates/argus-tool/Cargo.toml`
- Modify `crates/argus-approval/src/policy.rs`
- Modify `crates/argus-approval/src/hook.rs`
- Add tests in `crates/argus-tool/tests/knowledge_create_pr.rs`

## Assumptions

- GitHub CLI authentication is already handled outside this feature.
- The outer LLM will prepare documentation contents and manifest patches.
- `v1` only needs GitHub support.
- The repository default for a newly created manifest path is `.knowledge/repo.json`.
