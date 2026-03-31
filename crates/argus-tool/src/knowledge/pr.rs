use std::path::{Component, Path, PathBuf};
use std::process::Output;

use async_trait::async_trait;
use tempfile::TempDir;
use tokio::fs;
use tokio::process::Command;

use super::error::KnowledgeToolError;
use super::manifest::DEFAULT_MANIFEST_PATHS;
use super::manifest::{FileOverride, NodeOverride, RepositoryManifest, RepositoryManifestMeta};
use super::models::{
    KnowledgeCreatePrArgs, KnowledgeCreatePrResult, KnowledgeManifestFilePatch,
    KnowledgeManifestNodePatch, KnowledgeManifestPatch, KnowledgeManifestRepoPatch,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPrOutcome {
    pub pr_url: String,
    pub reused_existing: bool,
}

#[async_trait]
pub trait GitPrExecutor: Send + Sync {
    async fn ensure_auth(&self) -> Result<(), KnowledgeToolError>;

    async fn clone_repo(
        &self,
        target_repo: &str,
        destination: &Path,
    ) -> Result<(), KnowledgeToolError>;

    async fn prepare_branch(
        &self,
        repo_dir: &Path,
        base_ref: &str,
        branch: &str,
    ) -> Result<(), KnowledgeToolError>;

    async fn commit_and_push(
        &self,
        repo_dir: &Path,
        branch: &str,
        commit_message: &str,
    ) -> Result<String, KnowledgeToolError>;

    async fn create_or_reuse_pr(
        &self,
        repo_dir: &Path,
        base_ref: &str,
        branch: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<GitPrOutcome, KnowledgeToolError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CliGitPrExecutor;

impl CliGitPrExecutor {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    async fn run_command_output(
        &self,
        repo_dir: Option<&Path>,
        program: &str,
        args: &[&str],
    ) -> Result<Output, KnowledgeToolError> {
        let mut command = Command::new(program);
        command.args(args);
        if let Some(repo_dir) = repo_dir {
            command.current_dir(repo_dir);
        }

        command.output().await.map_err(|err| {
            KnowledgeToolError::RequestFailed(format!(
                "failed to run {}: {}",
                render_command(program, args),
                err
            ))
        })
    }

    async fn run_command(
        &self,
        repo_dir: Option<&Path>,
        program: &str,
        args: &[&str],
    ) -> Result<String, KnowledgeToolError> {
        let output = self.run_command_output(repo_dir, program, args).await?;
        if !output.status.success() {
            return Err(command_failed(program, args, &output));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    async fn remote_branch_exists(
        &self,
        repo_dir: &Path,
        branch: &str,
    ) -> Result<bool, KnowledgeToolError> {
        let branch_ref = format!("refs/heads/{branch}");
        let args = ["ls-remote", "--heads", "origin", branch_ref.as_str()];
        let output = self
            .run_command_output(Some(repo_dir), "git", &args)
            .await?;
        if !output.status.success() {
            return Err(command_failed("git", &args, &output));
        }

        Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
    }

    async fn has_pending_changes(&self, repo_dir: &Path) -> Result<bool, KnowledgeToolError> {
        let args = ["status", "--short"];
        let output = self
            .run_command_output(Some(repo_dir), "git", &args)
            .await?;
        if !output.status.success() {
            return Err(command_failed("git", &args, &output));
        }

        Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
    }
}

fn command_failed(program: &str, args: &[&str], output: &Output) -> KnowledgeToolError {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let details = if !stderr.is_empty() { stderr } else { stdout };
    KnowledgeToolError::RequestFailed(format!(
        "{} failed: {}",
        render_command(program, args),
        details
    ))
}

async fn resolve_repo_destination(
    repo_dir: &Path,
    relative_path: &str,
) -> Result<PathBuf, KnowledgeToolError> {
    let repo_root = fs::canonicalize(repo_dir).await.map_err(|err| {
        KnowledgeToolError::RequestFailed(format!(
            "failed to resolve checkout root {}: {}",
            repo_dir.display(),
            err
        ))
    })?;
    let mut current = repo_root.clone();

    for component in Path::new(relative_path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                current.push(part);
                match fs::symlink_metadata(&current).await {
                    Ok(metadata) => {
                        if metadata.file_type().is_symlink() {
                            return Err(KnowledgeToolError::invalid_arguments(format!(
                                "repo-relative path must not traverse symlinks: {relative_path}"
                            )));
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
                    Err(err) => {
                        return Err(KnowledgeToolError::RequestFailed(format!(
                            "failed to inspect {}: {}",
                            relative_path, err
                        )));
                    }
                }
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(KnowledgeToolError::invalid_arguments(format!(
                    "repo-relative path must stay inside checkout: {relative_path}"
                )));
            }
        }
    }

    Ok(repo_root.join(relative_path))
}

#[async_trait]
impl GitPrExecutor for CliGitPrExecutor {
    async fn ensure_auth(&self) -> Result<(), KnowledgeToolError> {
        self.run_command(None, "gh", &["auth", "status"]).await?;
        Ok(())
    }

    async fn clone_repo(
        &self,
        target_repo: &str,
        destination: &Path,
    ) -> Result<(), KnowledgeToolError> {
        let destination = destination.to_string_lossy().to_string();
        self.run_command(None, "gh", &["repo", "clone", target_repo, &destination])
            .await?;
        Ok(())
    }

    async fn prepare_branch(
        &self,
        repo_dir: &Path,
        base_ref: &str,
        branch: &str,
    ) -> Result<(), KnowledgeToolError> {
        self.run_command(Some(repo_dir), "git", &["fetch", "origin", base_ref])
            .await?;
        if self.remote_branch_exists(repo_dir, branch).await? {
            self.run_command(Some(repo_dir), "git", &["fetch", "origin", branch])
                .await?;
            let remote_branch = format!("origin/{branch}");
            self.run_command(
                Some(repo_dir),
                "git",
                &["checkout", "-B", branch, &remote_branch],
            )
            .await?;
        } else {
            let remote_base = format!("origin/{base_ref}");
            self.run_command(
                Some(repo_dir),
                "git",
                &["checkout", "-B", base_ref, &remote_base],
            )
            .await?;
            self.run_command(Some(repo_dir), "git", &["checkout", "-B", branch])
                .await?;
        }
        Ok(())
    }

    async fn commit_and_push(
        &self,
        repo_dir: &Path,
        branch: &str,
        commit_message: &str,
    ) -> Result<String, KnowledgeToolError> {
        self.run_command(Some(repo_dir), "git", &["add", "--all"])
            .await?;
        if self.has_pending_changes(repo_dir).await? {
            self.run_command(Some(repo_dir), "git", &["commit", "-m", commit_message])
                .await?;
        }
        self.run_command(
            Some(repo_dir),
            "git",
            &["push", "--set-upstream", "origin", branch],
        )
        .await?;
        self.run_command(Some(repo_dir), "git", &["rev-parse", "HEAD"])
            .await
    }

    async fn create_or_reuse_pr(
        &self,
        repo_dir: &Path,
        base_ref: &str,
        branch: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<GitPrOutcome, KnowledgeToolError> {
        let existing = self
            .run_command(
                Some(repo_dir),
                "gh",
                &["pr", "view", branch, "--json", "url", "--jq", ".url"],
            )
            .await;
        if let Ok(pr_url) = existing
            && !pr_url.is_empty()
        {
            return Ok(GitPrOutcome {
                pr_url,
                reused_existing: true,
            });
        }

        let mut args = vec![
            "pr", "create", "--base", base_ref, "--head", branch, "--title", title, "--body", body,
        ];
        if draft {
            args.push("--draft");
        }

        let pr_url = self.run_command(Some(repo_dir), "gh", &args).await?;
        Ok(GitPrOutcome {
            pr_url,
            reused_existing: false,
        })
    }
}

pub struct KnowledgePrService<E = CliGitPrExecutor> {
    executor: E,
}

#[async_trait]
pub trait KnowledgePrRuntime: Send + Sync {
    async fn create_pr(
        &self,
        args: &KnowledgeCreatePrArgs,
    ) -> Result<KnowledgeCreatePrResult, KnowledgeToolError>;
}

impl Default for KnowledgePrService<CliGitPrExecutor> {
    fn default() -> Self {
        Self::new()
    }
}

impl KnowledgePrService<CliGitPrExecutor> {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_executor(CliGitPrExecutor::new())
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

        let checkout = TempDir::new().map_err(|err| {
            KnowledgeToolError::RequestFailed(format!("failed to create temp checkout: {err}"))
        })?;
        self.executor
            .clone_repo(&args.target_repo, checkout.path())
            .await?;

        let base_ref = args.base_ref.clone().unwrap_or_else(|| "main".to_string());
        let branch = args
            .branch
            .clone()
            .unwrap_or_else(|| "codex/knowledge-pr-update".to_string());
        self.executor
            .prepare_branch(checkout.path(), &base_ref, &branch)
            .await?;

        let write_summary = write_requested_files(checkout.path(), args).await?;
        let commit_sha = self
            .executor
            .commit_and_push(checkout.path(), &branch, "docs: update knowledge base")
            .await?;
        let pr_outcome = self
            .executor
            .create_or_reuse_pr(
                checkout.path(),
                &base_ref,
                &branch,
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
            target_repo: args.target_repo.clone(),
            base_ref,
            branch,
            commit_sha,
            pr_url: pr_outcome.pr_url,
            manifest_path,
            changed_files: write_summary.changed_files.clone(),
            created_files: write_summary.created_files.clone(),
            updated_files: write_summary.updated_files.clone(),
            summary: format!(
                "{action} for {} with {} changed files",
                args.target_repo,
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

async fn write_requested_files(
    repo_dir: &Path,
    args: &KnowledgeCreatePrArgs,
) -> Result<WriteSummary, KnowledgeToolError> {
    let mut summary = WriteSummary::default();

    for file in &args.files {
        validate_repo_relative_path(&file.path)?;
        write_repo_file(repo_dir, &file.path, &file.content, &mut summary).await?;
    }

    let manifest_path = resolve_manifest_path(repo_dir, args.manifest.as_ref())?;
    summary.manifest_path = manifest_path.clone();
    if let (Some(manifest_path), Some(manifest_patch)) = (manifest_path, args.manifest.as_ref()) {
        let existing = if repo_dir.join(&manifest_path).exists() {
            Some(read_manifest(repo_dir, &manifest_path).await?)
        } else {
            None
        };
        let merged = merge_manifest(existing, manifest_patch)?;
        let serialized = serialize_manifest(&merged)?;
        write_repo_file(repo_dir, &manifest_path, &serialized, &mut summary).await?;
    }

    Ok(summary)
}

fn resolve_manifest_path(
    repo_dir: &Path,
    patch: Option<&KnowledgeManifestPatch>,
) -> Result<Option<String>, KnowledgeToolError> {
    if let Some(path) = patch.and_then(|patch| patch.path.clone()) {
        validate_repo_relative_path(&path)?;
        return Ok(Some(path));
    }

    for manifest_path in DEFAULT_MANIFEST_PATHS {
        if repo_dir.join(manifest_path).exists() {
            return Ok(Some((*manifest_path).to_string()));
        }
    }

    if patch.is_some() {
        return Ok(Some(DEFAULT_MANIFEST_PATHS[0].to_string()));
    }

    Ok(None)
}

async fn read_manifest(
    repo_dir: &Path,
    manifest_path: &str,
) -> Result<RepositoryManifest, KnowledgeToolError> {
    let manifest_text = fs::read_to_string(repo_dir.join(manifest_path))
        .await
        .map_err(|err| {
            KnowledgeToolError::RequestFailed(format!(
                "failed to read manifest {}: {}",
                manifest_path, err
            ))
        })?;
    let manifest_json = serde_json::from_str(&manifest_text)
        .map_err(|err| KnowledgeToolError::manifest_parse(err.to_string()))?;
    RepositoryManifest::from_json(manifest_json)
}

async fn write_repo_file(
    repo_dir: &Path,
    relative_path: &str,
    content: &str,
    summary: &mut WriteSummary,
) -> Result<(), KnowledgeToolError> {
    let destination = resolve_repo_destination(repo_dir, relative_path).await?;
    let existed = fs::try_exists(&destination).await.map_err(|err| {
        KnowledgeToolError::RequestFailed(format!(
            "failed to inspect destination {}: {}",
            relative_path, err
        ))
    })?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).await.map_err(|err| {
            KnowledgeToolError::RequestFailed(format!(
                "failed to create parent directories for {}: {}",
                relative_path, err
            ))
        })?;
    }
    fs::write(&destination, content).await.map_err(|err| {
        KnowledgeToolError::RequestFailed(format!("failed to write {}: {}", relative_path, err))
    })?;

    summary.changed_files.push(relative_path.to_string());
    if existed {
        summary.updated_files.push(relative_path.to_string());
    } else {
        summary.created_files.push(relative_path.to_string());
    }

    Ok(())
}

fn render_command(program: &str, args: &[&str]) -> String {
    let joined = args.join(" ");
    if joined.is_empty() {
        program.to_string()
    } else {
        format!("{program} {joined}")
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
