use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;

use super::cli::{CliRunner, RealCliRunner};
use super::error::KnowledgeToolError;
use super::manifest::DEFAULT_MANIFEST_PATHS;
use super::manifest::{FileOverride, NodeOverride, RepositoryManifest, RepositoryManifestMeta};
use super::models::{
    GitHubTreeEntryKind, KnowledgeCreatePrArgs, KnowledgeCreatePrResult,
    KnowledgeManifestFilePatch, KnowledgeManifestNodePatch, KnowledgeManifestPatch,
    KnowledgeManifestRepoPatch,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPrOutcome {
    pub pr_url: String,
    pub reused_existing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgePrWorkspaceFile {
    pub original_content: Option<String>,
    pub current_content: String,
    pub original_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgePrRemoteEntry {
    pub sha: String,
    pub mode: Option<String>,
    pub kind: GitHubTreeEntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgePrWorkspace {
    pub target_repo: String,
    pub owner: String,
    pub repo: String,
    pub base_ref: String,
    pub branch: String,
    pub branch_exists: bool,
    pub head_commit_sha: String,
    pub head_tree_sha: String,
    pub files: BTreeMap<String, KnowledgePrWorkspaceFile>,
    pub remote_entries: HashMap<String, KnowledgePrRemoteEntry>,
    pub work_dir: Option<PathBuf>,
}

#[async_trait]
pub trait GitPrExecutor: Send + Sync {
    async fn ensure_auth(&self) -> Result<(), KnowledgeToolError>;

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

// ---------------------------------------------------------------------------
// CliPrExecutor — uses `git` and `gh` CLI commands
// ---------------------------------------------------------------------------

pub struct CliPrExecutor<R: CliRunner = RealCliRunner> {
    runner: R,
    temp_dir: tokio::sync::Mutex<Option<tempfile::TempDir>>,
}

impl Default for CliPrExecutor<RealCliRunner> {
    fn default() -> Self {
        Self::new()
    }
}

impl CliPrExecutor<RealCliRunner> {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_runner(RealCliRunner)
    }
}

impl<R: CliRunner> CliPrExecutor<R> {
    #[must_use]
    pub fn new_with_runner(runner: R) -> Self {
        Self {
            runner,
            temp_dir: tokio::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl<R: CliRunner + 'static> GitPrExecutor for CliPrExecutor<R> {
    async fn ensure_auth(&self) -> Result<(), KnowledgeToolError> {
        let tmp = tempfile::TempDir::new()
            .map_err(|e| KnowledgeToolError::RequestFailed(e.to_string()))?;
        let tmp_path = tmp.path().to_path_buf();

        self.runner
            .run("gh", &["auth", "status"], &tmp_path)
            .await
            .map_err(|_| {
                KnowledgeToolError::RequestFailed(
                    "gh auth status failed: run `gh auth login` first".to_string(),
                )
            })?;

        self.runner
            .run("git", &["--version"], &tmp_path)
            .await
            .map_err(|_| {
                KnowledgeToolError::RequestFailed("git is not installed or not on PATH".to_string())
            })?;

        Ok(())
    }

    async fn prepare_workspace(
        &self,
        args: &KnowledgeCreatePrArgs,
    ) -> Result<KnowledgePrWorkspace, KnowledgeToolError> {
        let (owner, repo) = parse_target_repo(&args.target_repo)?;
        let base_ref = args.base_ref.clone().unwrap_or_else(|| "main".to_string());
        let branch = args
            .branch
            .clone()
            .unwrap_or_else(|| "codex/knowledge-pr-update".to_string());

        let tmp = tempfile::TempDir::new()
            .map_err(|e| KnowledgeToolError::RequestFailed(e.to_string()))?;
        let tmp_path = tmp.path().to_path_buf();
        let clone_url = format!("https://github.com/{owner}/{repo}");

        self.runner
            .run(
                "git",
                &[
                    "clone",
                    "--single-branch",
                    "--branch",
                    &base_ref,
                    "--depth=1",
                    &clone_url,
                    tmp_path.to_str().unwrap_or("."),
                ],
                Path::new("."),
            )
            .await?;

        let branch_exists = match self
            .runner
            .run(
                "git",
                &["ls-remote", "--heads", &clone_url, &branch],
                Path::new("."),
            )
            .await
        {
            Ok(output) => !output.stdout.trim().is_empty(),
            Err(_) => false,
        };

        if branch_exists {
            self.runner
                .run("git", &["fetch", "origin", &format!("{branch}:{branch}")], &tmp_path)
                .await?;
            self.runner
                .run("git", &["checkout", &branch], &tmp_path)
                .await?;
        }

        let ls_tree_output = self
            .runner
            .run("git", &["ls-tree", "-r", "HEAD"], &tmp_path)
            .await?;
        let mut remote_entries = HashMap::new();
        for line in ls_tree_output.stdout.lines() {
            let parts: Vec<&str> = line.splitn(4, &['\t', ' '][..]).collect();
            if parts.len() >= 4 {
                let mode = parts[0].to_string();
                let kind_str = parts[1];
                let sha = parts[2].to_string();
                let path = parts[3].to_string();
                let kind = if kind_str == "tree" {
                    GitHubTreeEntryKind::Tree
                } else {
                    GitHubTreeEntryKind::Blob
                };
                remote_entries.insert(
                    path,
                    KnowledgePrRemoteEntry {
                        sha,
                        mode: Some(mode),
                        kind,
                    },
                );
            }
        }

        let head_commit_sha = self
            .runner
            .run("git", &["rev-parse", "HEAD"], &tmp_path)
            .await?
            .stdout
            .trim()
            .to_string();
        let head_tree_sha = self
            .runner
            .run("git", &["rev-parse", "HEAD^{tree}"], &tmp_path)
            .await?
            .stdout
            .trim()
            .to_string();

        let mut files = BTreeMap::new();
        for path in relevant_workspace_paths(args)? {
            if let Some(entry) = remote_entries.get(&path)
                && entry.kind == GitHubTreeEntryKind::Blob
            {
                let content = self
                    .runner
                    .run("git", &["show", &format!("HEAD:{path}")], &tmp_path)
                    .await
                    .map(|out| out.stdout)
                    .unwrap_or_default();
                files.insert(
                    path,
                    KnowledgePrWorkspaceFile {
                        original_content: Some(content.clone()),
                        current_content: content,
                        original_mode: entry.mode.clone(),
                    },
                );
            }
        }

        let mut guard = self.temp_dir.lock().await;
        *guard = Some(tmp);

        Ok(KnowledgePrWorkspace {
            target_repo: args.target_repo.clone(),
            owner,
            repo,
            base_ref,
            branch,
            branch_exists,
            head_commit_sha,
            head_tree_sha,
            files,
            remote_entries,
            work_dir: Some(tmp_path),
        })
    }

    async fn commit_and_push(
        &self,
        workspace: &mut KnowledgePrWorkspace,
        commit_message: &str,
    ) -> Result<String, KnowledgeToolError> {
        let work_dir = workspace
            .work_dir
            .as_ref()
            .ok_or_else(|| {
                KnowledgeToolError::RequestFailed("workspace has no work_dir".to_string())
            })?;

        let has_changes = workspace
            .files
            .iter()
            .any(|(_, f)| f.original_content.as_deref() != Some(f.current_content.as_str()));

        if !workspace.branch_exists {
            self.runner
                .run("git", &["checkout", "-b", &workspace.branch], work_dir)
                .await?;
            workspace.branch_exists = true;
        }

        if has_changes {
            for (path, file) in &workspace.files {
                if file.original_content.as_deref() == Some(file.current_content.as_str()) {
                    continue;
                }
                let full_path = work_dir.join(path);
                if let Some(parent) = full_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| KnowledgeToolError::RequestFailed(e.to_string()))?;
                }
                std::fs::write(&full_path, &file.current_content)
                    .map_err(|e| KnowledgeToolError::RequestFailed(e.to_string()))?;
            }

            self.runner
                .run("git", &["add", "-A"], work_dir)
                .await?;
            self.runner
                .run(
                    "git",
                    &[
                        "-c",
                        "user.name=argus[bot]",
                        "-c",
                        "user.email=argus[bot]@users.noreply.github.com",
                        "commit",
                        "-m",
                        commit_message,
                    ],
                    work_dir,
                )
                .await?;
        }

        self.runner
            .run("git", &["push", "origin", &workspace.branch], work_dir)
            .await?;

        let commit_sha = self
            .runner
            .run("git", &["rev-parse", "HEAD"], work_dir)
            .await?
            .stdout
            .trim()
            .to_string();
        let tree_sha = self
            .runner
            .run("git", &["rev-parse", "HEAD^{tree}"], work_dir)
            .await?
            .stdout
            .trim()
            .to_string();

        workspace.head_commit_sha = commit_sha.clone();
        workspace.head_tree_sha = tree_sha;
        for file in workspace.files.values_mut() {
            file.original_content = Some(file.current_content.clone());
            if file.original_mode.is_none() {
                file.original_mode = Some("100644".to_string());
            }
        }

        Ok(commit_sha)
    }

    async fn create_or_reuse_pr(
        &self,
        workspace: &KnowledgePrWorkspace,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<GitPrOutcome, KnowledgeToolError> {
        let repo_flag = format!("{}/{}", workspace.owner, workspace.repo);
        let list_output = self
            .runner
            .run(
                "gh",
                &[
                    "pr",
                    "list",
                    "--repo",
                    &repo_flag,
                    "--head",
                    &workspace.branch,
                    "--state",
                    "open",
                    "--json",
                    "url",
                    "--limit",
                    "1",
                ],
                Path::new("."),
            )
            .await?;

        let urls: Vec<serde_json::Value> = serde_json::from_str(list_output.stdout.trim())
            .unwrap_or_default();
        if let Some(url_val) = urls.first().and_then(|v| v.get("url"))
            && let Some(pr_url) = url_val.as_str()
        {
            return Ok(GitPrOutcome {
                pr_url: pr_url.to_string(),
                reused_existing: true,
            });
        }

        let mut pr_args = vec![
            "pr".to_string(),
            "create".to_string(),
            "--repo".to_string(),
            repo_flag.clone(),
            "--base".to_string(),
            workspace.base_ref.clone(),
            "--head".to_string(),
            workspace.branch.clone(),
            "--title".to_string(),
            title.to_string(),
            "--body".to_string(),
            body.to_string(),
        ];
        if draft {
            pr_args.push("--draft".to_string());
        }

        let args_strs: Vec<&str> = pr_args.iter().map(String::as_str).collect();
        let create_output = self
            .runner
            .run("gh", &args_strs, Path::new("."))
            .await?;

        let pr_url = create_output
            .stdout
            .trim()
            .lines()
            .last()
            .unwrap_or("")
            .to_string();

        Ok(GitPrOutcome {
            pr_url,
            reused_existing: false,
        })
    }
}

pub struct KnowledgePrService<E = CliPrExecutor<RealCliRunner>> {
    executor: E,
}

#[async_trait]
pub trait KnowledgePrRuntime: Send + Sync {
    async fn create_pr(
        &self,
        args: &KnowledgeCreatePrArgs,
    ) -> Result<KnowledgeCreatePrResult, KnowledgeToolError>;
}

impl Default for KnowledgePrService<CliPrExecutor<RealCliRunner>> {
    fn default() -> Self {
        Self::new()
    }
}

impl KnowledgePrService<CliPrExecutor<RealCliRunner>> {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_executor(CliPrExecutor::new())
    }
}

impl<E: GitPrExecutor> KnowledgePrService<E> {
    #[must_use]
    pub fn new_with_executor(executor: E) -> Self {
        Self { executor }
    }

    pub async fn create_pr(
        &self,
        args: &KnowledgeCreatePrArgs,
    ) -> Result<KnowledgeCreatePrResult, KnowledgeToolError> {
        self.executor.ensure_auth().await?;

        let mut workspace = self.executor.prepare_workspace(args).await?;
        let write_summary = write_requested_files(&mut workspace, args)?;
        let commit_sha = self
            .executor
            .commit_and_push(&mut workspace, "docs: update knowledge base")
            .await?;
        let pr_outcome = self
            .executor
            .create_or_reuse_pr(
                &workspace,
                &args.pr_title,
                &args.pr_body,
                args.draft.unwrap_or(false),
            )
            .await?;

        let action = if pr_outcome.reused_existing {
            "Updated existing PR"
        } else if args.draft.unwrap_or(false) {
            "Opened draft PR"
        } else {
            "Opened PR"
        };
        let manifest_path = write_summary
            .manifest_path
            .unwrap_or_else(|| DEFAULT_MANIFEST_PATHS[0].to_string());

        Ok(KnowledgeCreatePrResult {
            target_repo: workspace.target_repo.clone(),
            base_ref: workspace.base_ref.clone(),
            branch: workspace.branch.clone(),
            commit_sha,
            pr_url: pr_outcome.pr_url,
            manifest_path,
            changed_files: write_summary.changed_files.clone(),
            created_files: write_summary.created_files.clone(),
            updated_files: write_summary.updated_files.clone(),
            summary: format!(
                "{action} for {} with {} changed files",
                workspace.target_repo,
                write_summary.changed_files.len()
            ),
        })
    }
}

#[async_trait]
impl<E: GitPrExecutor> KnowledgePrRuntime for KnowledgePrService<E> {
    async fn create_pr(
        &self,
        args: &KnowledgeCreatePrArgs,
    ) -> Result<KnowledgeCreatePrResult, KnowledgeToolError> {
        KnowledgePrService::create_pr(self, args).await
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct WriteSummary {
    changed_files: Vec<String>,
    created_files: Vec<String>,
    updated_files: Vec<String>,
    manifest_path: Option<String>,
}

fn write_requested_files(
    workspace: &mut KnowledgePrWorkspace,
    args: &KnowledgeCreatePrArgs,
) -> Result<WriteSummary, KnowledgeToolError> {
    let mut summary = WriteSummary::default();

    for file in &args.files {
        write_repo_file(workspace, &file.path, &file.content, &mut summary)?;
    }

    let manifest_path = resolve_manifest_path(workspace, args.manifest.as_ref())?;
    summary.manifest_path = manifest_path.clone();
    if let (Some(manifest_path), Some(manifest_patch)) = (manifest_path, args.manifest.as_ref()) {
        let existing = workspace
            .files
            .contains_key(&manifest_path)
            .then(|| read_manifest(workspace, &manifest_path))
            .transpose()?;
        let merged = merge_manifest(existing, manifest_patch)?;
        let serialized = serialize_manifest(&merged)?;
        write_repo_file(workspace, &manifest_path, &serialized, &mut summary)?;
    }

    Ok(summary)
}

fn resolve_manifest_path(
    workspace: &KnowledgePrWorkspace,
    patch: Option<&KnowledgeManifestPatch>,
) -> Result<Option<String>, KnowledgeToolError> {
    if let Some(path) = patch.and_then(|patch| patch.path.clone()) {
        validate_workspace_path(workspace, &path)?;
        return Ok(Some(path));
    }

    for manifest_path in DEFAULT_MANIFEST_PATHS {
        if workspace.files.contains_key(*manifest_path) {
            return Ok(Some((*manifest_path).to_string()));
        }
    }

    if patch.is_some() {
        return Ok(Some(DEFAULT_MANIFEST_PATHS[0].to_string()));
    }

    Ok(None)
}

fn read_manifest(
    workspace: &KnowledgePrWorkspace,
    manifest_path: &str,
) -> Result<RepositoryManifest, KnowledgeToolError> {
    let manifest_text = workspace
        .files
        .get(manifest_path)
        .ok_or_else(|| KnowledgeToolError::NotFound(manifest_path.to_string()))?;
    let manifest_json = serde_json::from_str(&manifest_text.current_content)
        .map_err(|err| KnowledgeToolError::manifest_parse(err.to_string()))?;
    RepositoryManifest::from_json(manifest_json)
}

fn write_repo_file(
    workspace: &mut KnowledgePrWorkspace,
    relative_path: &str,
    content: &str,
    summary: &mut WriteSummary,
) -> Result<(), KnowledgeToolError> {
    validate_workspace_path(workspace, relative_path)?;
    let existed = workspace
        .files
        .get(relative_path)
        .map(|file| file.original_content.is_some())
        .unwrap_or(false);

    workspace
        .files
        .entry(relative_path.to_string())
        .and_modify(|file| file.current_content = content.to_string())
        .or_insert_with(|| KnowledgePrWorkspaceFile {
            original_content: None,
            current_content: content.to_string(),
            original_mode: None,
        });

    summary.changed_files.push(relative_path.to_string());
    if existed {
        summary.updated_files.push(relative_path.to_string());
    } else {
        summary.created_files.push(relative_path.to_string());
    }

    Ok(())
}

fn relevant_workspace_paths(
    args: &KnowledgeCreatePrArgs,
) -> Result<BTreeSet<String>, KnowledgeToolError> {
    let mut paths = BTreeSet::new();

    for file in &args.files {
        validate_repo_relative_path(&file.path)?;
        paths.insert(file.path.clone());
    }

    if let Some(manifest) = &args.manifest {
        if let Some(path) = &manifest.path {
            validate_repo_relative_path(path)?;
            paths.insert(path.clone());
        }
        for manifest_path in DEFAULT_MANIFEST_PATHS {
            paths.insert((*manifest_path).to_string());
        }
    }

    Ok(paths)
}

fn validate_workspace_path(
    workspace: &KnowledgePrWorkspace,
    path: &str,
) -> Result<(), KnowledgeToolError> {
    validate_repo_relative_path(path)?;

    let mut current = String::new();
    for (index, segment) in Path::new(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .enumerate()
    {
        if index > 0 {
            current.push('/');
        }
        current.push_str(&segment);

        if let Some(entry) = workspace.remote_entries.get(&current) {
            if entry.mode.as_deref() == Some("120000") {
                return Err(KnowledgeToolError::invalid_arguments(format!(
                    "repo-relative path must not traverse symlinks: {path}"
                )));
            }

            let is_exact = current == path;
            if !is_exact && entry.kind != GitHubTreeEntryKind::Tree {
                return Err(KnowledgeToolError::invalid_arguments(format!(
                    "repo-relative path must not traverse files: {path}"
                )));
            }
            if is_exact && entry.kind == GitHubTreeEntryKind::Tree {
                return Err(KnowledgeToolError::invalid_arguments(format!(
                    "repo-relative path points to a directory: {path}"
                )));
            }
        }
    }

    Ok(())
}

fn parse_target_repo(target_repo: &str) -> Result<(String, String), KnowledgeToolError> {
    let trimmed = target_repo.trim();
    let mut parts = trimmed.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(repo), None) if !owner.trim().is_empty() && !repo.trim().is_empty() => {
            Ok((owner.to_string(), repo.to_string()))
        }
        _ => Err(KnowledgeToolError::invalid_arguments(
            "target_repo must be in owner/name format for action create_knowledge_pr",
        )),
    }
}

pub fn validate_repo_relative_path(path: &str) -> Result<(), KnowledgeToolError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(KnowledgeToolError::invalid_arguments(
            "repo-relative path must not be empty",
        ));
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return Err(KnowledgeToolError::invalid_arguments(format!(
            "absolute paths are not allowed: {trimmed}"
        )));
    }

    for component in candidate.components() {
        match component {
            Component::ParentDir => {
                return Err(KnowledgeToolError::invalid_arguments(format!(
                    "repo-relative path must not contain '..': {trimmed}"
                )));
            }
            Component::Normal(part) if part == ".git" => {
                return Err(KnowledgeToolError::invalid_arguments(format!(
                    "repo-relative path must not traverse .git: {trimmed}"
                )));
            }
            _ => {}
        }
    }

    Ok(())
}

pub fn merge_manifest(
    existing: Option<RepositoryManifest>,
    patch: &KnowledgeManifestPatch,
) -> Result<RepositoryManifest, KnowledgeToolError> {
    if let Some(path) = patch.path.as_deref() {
        validate_repo_relative_path(path)?;
    }

    let mut manifest = existing.unwrap_or(RepositoryManifest {
        version: 1,
        repo: None,
        files: Vec::new(),
        nodes: Vec::new(),
    });

    if let Some(repo_patch) = &patch.repo {
        merge_repo(&mut manifest.repo, repo_patch)?;
    }

    if let Some(files) = &patch.files {
        for file in files {
            validate_repo_relative_path(&file.path)?;
            upsert_file(&mut manifest.files, file);
        }
    }

    if let Some(nodes) = &patch.nodes {
        for node in nodes {
            validate_repo_relative_path(&node.source.path)?;
            upsert_node(&mut manifest.nodes, node);
        }
    }

    Ok(manifest)
}

pub fn serialize_manifest(manifest: &RepositoryManifest) -> Result<String, KnowledgeToolError> {
    let mut output = String::new();
    write_manifest(&mut output, manifest, 0)?;
    Ok(output)
}

fn merge_repo(
    current: &mut Option<RepositoryManifestMeta>,
    patch: &KnowledgeManifestRepoPatch,
) -> Result<(), KnowledgeToolError> {
    let repo = current.get_or_insert_with(|| RepositoryManifestMeta {
        title: None,
        default_branch: None,
        include: Vec::new(),
        exclude: Vec::new(),
        entrypoints: Vec::new(),
    });

    if let Some(title) = &patch.title {
        repo.title = Some(title.clone());
    }

    if let Some(default_branch) = &patch.default_branch {
        repo.default_branch = Some(default_branch.clone());
    }

    if let Some(include) = &patch.include {
        repo.include = merge_unique_strings(&repo.include, include);
    }

    if let Some(exclude) = &patch.exclude {
        repo.exclude = merge_unique_strings(&repo.exclude, exclude);
    }

    if let Some(entrypoints) = &patch.entrypoints {
        repo.entrypoints = merge_unique_strings(&repo.entrypoints, entrypoints);
    }

    Ok(())
}

fn upsert_file(files: &mut Vec<FileOverride>, patch: &KnowledgeManifestFilePatch) {
    if let Some(existing) = files.iter_mut().find(|file| file.path == patch.path) {
        if let Some(title) = &patch.title {
            existing.title = Some(title.clone());
        }
        if let Some(summary) = &patch.summary {
            existing.summary = Some(summary.clone());
        }
        if let Some(tags) = &patch.tags {
            existing.tags = tags.clone();
        }
        if let Some(aliases) = &patch.aliases {
            existing.aliases = aliases.clone();
        }
    } else {
        files.push(FileOverride {
            path: patch.path.clone(),
            title: patch.title.clone(),
            summary: patch.summary.clone(),
            tags: patch.tags.clone().unwrap_or_default(),
            aliases: patch.aliases.clone().unwrap_or_default(),
        });
    }
}

fn upsert_node(nodes: &mut Vec<NodeOverride>, patch: &KnowledgeManifestNodePatch) {
    if let Some(existing) = nodes.iter_mut().find(|node| node.id == patch.id) {
        existing.source.path = patch.source.path.clone();
        if let Some(heading) = &patch.source.heading {
            existing.source.heading = Some(heading.clone());
        }
        if let Some(title) = &patch.title {
            existing.title = Some(title.clone());
        }
        if let Some(summary) = &patch.summary {
            existing.summary = Some(summary.clone());
        }
        if let Some(tags) = &patch.tags {
            existing.tags = tags.clone();
        }
        if let Some(aliases) = &patch.aliases {
            existing.aliases = aliases.clone();
        }
        if let Some(relations) = &patch.relations {
            existing.relations = relations.clone();
        }
    } else {
        nodes.push(NodeOverride {
            id: patch.id.clone(),
            source: super::manifest::NodeSource {
                path: patch.source.path.clone(),
                heading: patch.source.heading.clone(),
            },
            title: patch.title.clone(),
            summary: patch.summary.clone(),
            tags: patch.tags.clone().unwrap_or_default(),
            aliases: patch.aliases.clone().unwrap_or_default(),
            relations: patch.relations.clone().unwrap_or_default(),
        });
    }
}

fn merge_unique_strings(existing: &[String], patch: &[String]) -> Vec<String> {
    let mut merged = Vec::with_capacity(existing.len() + patch.len());

    for value in existing.iter().chain(patch.iter()) {
        if !merged.iter().any(|seen| seen == value) {
            merged.push(value.clone());
        }
    }

    merged
}

fn write_manifest(
    output: &mut String,
    manifest: &RepositoryManifest,
    indent: usize,
) -> Result<(), KnowledgeToolError> {
    output.push_str("{\n");

    let mut first = true;
    write_field(output, indent + 1, &mut first, "version", |output| {
        output.push_str(&manifest.version.to_string());
        Ok(())
    })?;

    if let Some(repo) = &manifest.repo {
        write_field(output, indent + 1, &mut first, "repo", |output| {
            write_repo_meta(output, repo, indent + 1)
        })?;
    }

    write_field(output, indent + 1, &mut first, "files", |output| {
        write_file_overrides(output, &manifest.files, indent + 1)
    })?;

    write_field(output, indent + 1, &mut first, "nodes", |output| {
        write_node_overrides(output, &manifest.nodes, indent + 1)
    })?;

    output.push('\n');
    push_indent(output, indent);
    output.push('}');
    Ok(())
}

fn write_repo_meta(
    output: &mut String,
    repo: &RepositoryManifestMeta,
    indent: usize,
) -> Result<(), KnowledgeToolError> {
    output.push_str("{\n");

    let mut first = true;
    if let Some(title) = &repo.title {
        write_field(output, indent + 1, &mut first, "title", |output| {
            push_json_string(output, title)
        })?;
    }

    if let Some(default_branch) = &repo.default_branch {
        write_field(output, indent + 1, &mut first, "default_branch", |output| {
            push_json_string(output, default_branch)
        })?;
    }

    write_field(output, indent + 1, &mut first, "include", |output| {
        write_string_array(output, &repo.include, indent + 1)
    })?;
    write_field(output, indent + 1, &mut first, "exclude", |output| {
        write_string_array(output, &repo.exclude, indent + 1)
    })?;
    write_field(output, indent + 1, &mut first, "entrypoints", |output| {
        write_string_array(output, &repo.entrypoints, indent + 1)
    })?;

    output.push('\n');
    push_indent(output, indent);
    output.push('}');
    Ok(())
}

fn write_file_overrides(
    output: &mut String,
    files: &[FileOverride],
    indent: usize,
) -> Result<(), KnowledgeToolError> {
    write_array(output, files, indent, |output, file, item_indent| {
        write_file_override(output, file, item_indent)
    })
}

fn write_node_overrides(
    output: &mut String,
    nodes: &[NodeOverride],
    indent: usize,
) -> Result<(), KnowledgeToolError> {
    write_array(output, nodes, indent, |output, node, item_indent| {
        write_node_override(output, node, item_indent)
    })
}

fn write_file_override(
    output: &mut String,
    file: &FileOverride,
    indent: usize,
) -> Result<(), KnowledgeToolError> {
    output.push_str("{\n");

    let mut first = true;
    write_field(output, indent + 1, &mut first, "path", |output| {
        push_json_string(output, &file.path)
    })?;

    if let Some(title) = &file.title {
        write_field(output, indent + 1, &mut first, "title", |output| {
            push_json_string(output, title)
        })?;
    }

    if let Some(summary) = &file.summary {
        write_field(output, indent + 1, &mut first, "summary", |output| {
            push_json_string(output, summary)
        })?;
    }

    write_field(output, indent + 1, &mut first, "tags", |output| {
        write_string_array(output, &file.tags, indent + 1)
    })?;
    write_field(output, indent + 1, &mut first, "aliases", |output| {
        write_string_array(output, &file.aliases, indent + 1)
    })?;

    output.push('\n');
    push_indent(output, indent);
    output.push('}');
    Ok(())
}

fn write_node_override(
    output: &mut String,
    node: &NodeOverride,
    indent: usize,
) -> Result<(), KnowledgeToolError> {
    output.push_str("{\n");

    let mut first = true;
    write_field(output, indent + 1, &mut first, "id", |output| {
        push_json_string(output, &node.id)
    })?;
    write_field(output, indent + 1, &mut first, "source", |output| {
        write_node_source(output, &node.source, indent + 1)
    })?;

    if let Some(title) = &node.title {
        write_field(output, indent + 1, &mut first, "title", |output| {
            push_json_string(output, title)
        })?;
    }

    if let Some(summary) = &node.summary {
        write_field(output, indent + 1, &mut first, "summary", |output| {
            push_json_string(output, summary)
        })?;
    }

    write_field(output, indent + 1, &mut first, "tags", |output| {
        write_string_array(output, &node.tags, indent + 1)
    })?;
    write_field(output, indent + 1, &mut first, "aliases", |output| {
        write_string_array(output, &node.aliases, indent + 1)
    })?;
    write_field(output, indent + 1, &mut first, "relations", |output| {
        write_relations(output, &node.relations, indent + 1)
    })?;

    output.push('\n');
    push_indent(output, indent);
    output.push('}');
    Ok(())
}

fn write_node_source(
    output: &mut String,
    source: &super::manifest::NodeSource,
    indent: usize,
) -> Result<(), KnowledgeToolError> {
    output.push_str("{\n");

    let mut first = true;
    write_field(output, indent + 1, &mut first, "path", |output| {
        push_json_string(output, &source.path)
    })?;

    if let Some(heading) = &source.heading {
        write_field(output, indent + 1, &mut first, "heading", |output| {
            push_json_string(output, heading)
        })?;
    }

    output.push('\n');
    push_indent(output, indent);
    output.push('}');
    Ok(())
}

fn write_relations(
    output: &mut String,
    relations: &[super::models::KnowledgeRelation],
    indent: usize,
) -> Result<(), KnowledgeToolError> {
    write_array(
        output,
        relations,
        indent,
        |output, relation, item_indent| {
            output.push_str("{\n");
            let mut first = true;
            write_field(output, item_indent + 1, &mut first, "type", |output| {
                push_json_string(output, &relation.relation_type)
            })?;
            write_field(output, item_indent + 1, &mut first, "target", |output| {
                push_json_string(output, &relation.target)
            })?;
            output.push('\n');
            push_indent(output, item_indent);
            output.push('}');
            Ok(())
        },
    )
}

fn write_array<T, F>(
    output: &mut String,
    values: &[T],
    indent: usize,
    mut write_item: F,
) -> Result<(), KnowledgeToolError>
where
    F: FnMut(&mut String, &T, usize) -> Result<(), KnowledgeToolError>,
{
    if values.is_empty() {
        output.push_str("[]");
        return Ok(());
    }

    output.push_str("[\n");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            output.push_str(",\n");
        }
        push_indent(output, indent + 1);
        write_item(output, value, indent + 1)?;
    }
    output.push('\n');
    push_indent(output, indent);
    output.push(']');
    Ok(())
}

fn write_string_array(
    output: &mut String,
    values: &[String],
    indent: usize,
) -> Result<(), KnowledgeToolError> {
    if values.is_empty() {
        output.push_str("[]");
        return Ok(());
    }

    output.push_str("[\n");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            output.push_str(",\n");
        }
        push_indent(output, indent + 1);
        push_json_string(output, value)?;
    }
    output.push('\n');
    push_indent(output, indent);
    output.push(']');
    Ok(())
}

fn write_field<F>(
    output: &mut String,
    indent: usize,
    first: &mut bool,
    name: &str,
    writer: F,
) -> Result<(), KnowledgeToolError>
where
    F: FnOnce(&mut String) -> Result<(), KnowledgeToolError>,
{
    if *first {
        *first = false;
    } else {
        output.push_str(",\n");
    }
    push_indent(output, indent);
    push_json_string(output, name)?;
    output.push_str(": ");
    writer(output)
}

fn push_indent(output: &mut String, indent: usize) {
    for _ in 0..indent {
        output.push_str("  ");
    }
}

fn push_json_string(output: &mut String, value: &str) -> Result<(), KnowledgeToolError> {
    let rendered = serde_json::to_string(value)
        .map_err(|err| KnowledgeToolError::unexpected_response(err.to_string()))?;
    output.push_str(&rendered);
    Ok(())
}
