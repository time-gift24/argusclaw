use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tempfile::tempdir;

use argus_protocol::ids::ThreadId;
use argus_protocol::{NamedTool, ToolExecutionContext};
use argus_tool::knowledge::{
    CliGitPrExecutor, DefaultKnowledgeRuntime, GitHubBlob, GitHubSnapshot, GitHubTree,
    GitPrExecutor, GitPrOutcome, KnowledgeBackend, KnowledgeCreatePrArgs, KnowledgeCreatePrResult,
    KnowledgeManifestFilePatch, KnowledgeManifestNodePatch, KnowledgeManifestNodeSourcePatch,
    KnowledgeManifestPatch, KnowledgeManifestRepoPatch, KnowledgePrRuntime, KnowledgePrService,
    KnowledgeRepoDescriptor, KnowledgeRuntimeBackend, KnowledgeTool, KnowledgeToolError,
    RepositoryManifest, merge_manifest, serialize_manifest, validate_repo_relative_path,
};

fn sample_existing_manifest() -> RepositoryManifest {
    RepositoryManifest::from_json(serde_json::json!({
        "version": 1,
        "repo": {
            "title": "Old docs",
            "default_branch": "main",
            "include": ["docs", "docs"],
            "exclude": ["tmp"],
            "entrypoints": ["README.md", "README.md"]
        },
        "files": [
            {
                "path": "docs/guide.md",
                "title": "Old guide",
                "summary": "Old summary",
                "tags": ["legacy"],
                "aliases": ["guide"]
            }
        ],
        "nodes": [
            {
                "id": "docs/guide#intro",
                "source": {
                    "path": "docs/guide.md",
                    "heading": "Intro"
                },
                "title": "Old intro",
                "summary": "Old node",
                "tags": ["legacy"],
                "aliases": ["intro"],
                "relations": [
                    {
                        "type": "related",
                        "target": "docs/api#intro"
                    }
                ]
            }
        ]
    }))
    .expect("sample manifest should parse")
}

fn sample_patch() -> KnowledgeManifestPatch {
    KnowledgeManifestPatch {
        path: Some(".knowledge/repo.json".to_string()),
        repo: Some(KnowledgeManifestRepoPatch {
            title: Some("Docs".to_string()),
            default_branch: None,
            include: Some(vec![
                "docs".to_string(),
                "api".to_string(),
                "docs".to_string(),
            ]),
            exclude: Some(vec![
                "tmp".to_string(),
                "generated".to_string(),
                "tmp".to_string(),
            ]),
            entrypoints: Some(vec![
                "README.md".to_string(),
                "docs/guide.md".to_string(),
                "README.md".to_string(),
            ]),
        }),
        files: Some(vec![
            KnowledgeManifestFilePatch {
                path: "docs/guide.md".to_string(),
                title: Some("Guide".to_string()),
                summary: Some("Updated summary".to_string()),
                tags: Some(vec!["docs".to_string()]),
                aliases: Some(vec!["guide".to_string(), "start".to_string()]),
            },
            KnowledgeManifestFilePatch {
                path: "docs/api.md".to_string(),
                title: Some("API".to_string()),
                summary: None,
                tags: Some(vec!["api".to_string()]),
                aliases: Some(vec!["reference".to_string()]),
            },
        ]),
        nodes: Some(vec![
            KnowledgeManifestNodePatch {
                id: "docs/guide#intro".to_string(),
                source: KnowledgeManifestNodeSourcePatch {
                    path: "docs/guide.md".to_string(),
                    heading: Some("Intro".to_string()),
                },
                title: Some("Intro".to_string()),
                summary: Some("Updated node".to_string()),
                tags: Some(vec!["docs".to_string()]),
                aliases: Some(vec!["intro".to_string()]),
                relations: Some(vec![argus_tool::knowledge::KnowledgeRelation {
                    relation_type: "related".to_string(),
                    target: "docs/api#intro".to_string(),
                }]),
            },
            KnowledgeManifestNodePatch {
                id: "docs/api#overview".to_string(),
                source: KnowledgeManifestNodeSourcePatch {
                    path: "docs/api.md".to_string(),
                    heading: Some("Overview".to_string()),
                },
                title: Some("Overview".to_string()),
                summary: None,
                tags: Some(vec!["api".to_string()]),
                aliases: Some(vec!["reference".to_string()]),
                relations: None,
            },
        ]),
    }
}

fn sample_create_pr_args() -> KnowledgeCreatePrArgs {
    KnowledgeCreatePrArgs {
        target_repo: "acme/docs".to_string(),
        base_ref: Some("main".to_string()),
        branch: Some("codex/knowledge-bootstrap".to_string()),
        pr_title: "Bootstrap knowledge docs".to_string(),
        pr_body: "Adds knowledge docs and manifest.".to_string(),
        draft: Some(true),
        files: vec![argus_tool::knowledge::KnowledgeFileWrite {
            path: "docs/knowledge/README.md".to_string(),
            content: "# Knowledge\n".to_string(),
        }],
        manifest: Some(sample_patch()),
    }
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git_git_dir_output(git_dir: &Path, args: &[&str]) -> String {
    let git_dir = git_dir.to_string_lossy().to_string();
    let output = std::process::Command::new("git")
        .args(["--git-dir", &git_dir])
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git --git-dir={} {} failed\nstdout:\n{}\nstderr:\n{}",
        git_dir,
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn configure_git_identity(repo_dir: &Path) {
    run_git(repo_dir, &["config", "user.email", "test@example.com"]);
    run_git(repo_dir, &["config", "user.name", "Codex Test"]);
}

fn init_origin_with_main() -> tempfile::TempDir {
    let temp = tempdir().expect("tempdir should create");
    let origin = temp.path().join("origin.git");
    let origin_str = origin.to_string_lossy().to_string();
    run_git(temp.path(), &["init", "--bare", &origin_str]);
    let _ = git_git_dir_output(&origin, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let seed = temp.path().join("seed");
    let seed_str = seed.to_string_lossy().to_string();
    run_git(temp.path(), &["clone", &origin_str, &seed_str]);
    configure_git_identity(&seed);
    std::fs::write(seed.join("README.md"), "base\n").expect("seed file should write");
    run_git(&seed, &["add", "README.md"]);
    run_git(&seed, &["commit", "-m", "base"]);
    run_git(&seed, &["branch", "-M", "main"]);
    run_git(&seed, &["push", "origin", "main"]);

    temp
}

#[derive(Debug, Default)]
struct FakeExecutorState {
    calls: Vec<String>,
    seed_files: HashMap<String, String>,
    captured_files: HashMap<String, String>,
    auth_error: Option<String>,
    pr_error: Option<String>,
    existing_pr_url: Option<String>,
    created_pr_url: Option<String>,
    commit_sha: String,
}

#[derive(Clone, Default)]
struct FakeGitPrExecutor {
    state: Arc<Mutex<FakeExecutorState>>,
}

impl FakeGitPrExecutor {
    fn with_state(state: FakeExecutorState) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    fn capture_repo(repo_dir: &Path) -> HashMap<String, String> {
        fn walk(root: &Path, current: &Path, files: &mut HashMap<String, String>) {
            let entries = std::fs::read_dir(current).expect("directory should read");
            for entry in entries {
                let entry = entry.expect("entry should exist");
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, files);
                    continue;
                }

                let relative = path
                    .strip_prefix(root)
                    .expect("path should be inside repo")
                    .to_string_lossy()
                    .to_string();
                let content = std::fs::read_to_string(&path).expect("file should read");
                files.insert(relative, content);
            }
        }

        let mut files = HashMap::new();
        walk(repo_dir, repo_dir, &mut files);
        files
    }
}

#[async_trait]
impl GitPrExecutor for FakeGitPrExecutor {
    async fn ensure_auth(&self) -> Result<(), argus_tool::knowledge::KnowledgeToolError> {
        let mut state = self.state.lock().expect("state should lock");
        state.calls.push("ensure_auth".to_string());
        if let Some(error) = &state.auth_error {
            return Err(argus_tool::knowledge::KnowledgeToolError::RequestFailed(
                error.clone(),
            ));
        }
        Ok(())
    }

    async fn clone_repo(
        &self,
        _target_repo: &str,
        destination: &Path,
    ) -> Result<(), argus_tool::knowledge::KnowledgeToolError> {
        let mut state = self.state.lock().expect("state should lock");
        state.calls.push("clone_repo".to_string());
        std::fs::create_dir_all(destination).expect("destination should exist");
        for (path, content) in state.seed_files.clone() {
            let full_path = destination.join(&path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).expect("parent should exist");
            }
            std::fs::write(full_path, content).expect("seed file should write");
        }
        Ok(())
    }

    async fn prepare_branch(
        &self,
        _repo_dir: &Path,
        base_ref: &str,
        branch: &str,
    ) -> Result<(), argus_tool::knowledge::KnowledgeToolError> {
        let mut state = self.state.lock().expect("state should lock");
        state
            .calls
            .push(format!("prepare_branch:{base_ref}:{branch}"));
        Ok(())
    }

    async fn commit_and_push(
        &self,
        repo_dir: &Path,
        branch: &str,
        _commit_message: &str,
    ) -> Result<String, argus_tool::knowledge::KnowledgeToolError> {
        let mut state = self.state.lock().expect("state should lock");
        state.calls.push(format!("commit_and_push:{branch}"));
        state.captured_files = Self::capture_repo(repo_dir);
        Ok(state.commit_sha.clone())
    }

    async fn create_or_reuse_pr(
        &self,
        _repo_dir: &Path,
        base_ref: &str,
        branch: &str,
        _title: &str,
        _body: &str,
        draft: bool,
    ) -> Result<GitPrOutcome, argus_tool::knowledge::KnowledgeToolError> {
        let mut state = self.state.lock().expect("state should lock");
        state
            .calls
            .push(format!("create_or_reuse_pr:{base_ref}:{branch}:{draft}"));
        if let Some(error) = &state.pr_error {
            return Err(argus_tool::knowledge::KnowledgeToolError::RequestFailed(
                error.clone(),
            ));
        }
        if let Some(pr_url) = &state.existing_pr_url {
            return Ok(GitPrOutcome {
                pr_url: pr_url.clone(),
                reused_existing: true,
            });
        }

        Ok(GitPrOutcome {
            pr_url: state
                .created_pr_url
                .clone()
                .unwrap_or_else(|| "https://example.com/pr/1".to_string()),
            reused_existing: false,
        })
    }
}

fn make_ctx() -> Arc<ToolExecutionContext> {
    let (pipe_tx, _) = tokio::sync::broadcast::channel(16);
    let (control_tx, _control_rx) = tokio::sync::mpsc::unbounded_channel();
    Arc::new(ToolExecutionContext {
        thread_id: ThreadId::new(),
        agent_id: None,
        pipe_tx,
        control_tx,
    })
}

#[derive(Clone, Default)]
struct NoopKnowledgeBackend;

#[async_trait]
impl KnowledgeRuntimeBackend for NoopKnowledgeBackend {
    async fn list_repos(&self) -> Result<Vec<KnowledgeRepoDescriptor>, KnowledgeToolError> {
        Ok(Vec::new())
    }

    fn repo_descriptor(&self, _repo_id: &str) -> Option<KnowledgeRepoDescriptor> {
        None
    }

    async fn resolve_snapshot(
        &self,
        _repo_id: &str,
        _ref_name: &str,
    ) -> Result<(String, GitHubSnapshot), KnowledgeToolError> {
        panic!("read backend should not be used for create_knowledge_pr");
    }
}

#[async_trait]
impl KnowledgeBackend for NoopKnowledgeBackend {
    async fn read_tree(&self, _snapshot_id: &str) -> Result<GitHubTree, KnowledgeToolError> {
        panic!("read backend should not be used for create_knowledge_pr");
    }

    async fn read_manifest(
        &self,
        _snapshot_id: &str,
    ) -> Result<Option<RepositoryManifest>, KnowledgeToolError> {
        panic!("read backend should not be used for create_knowledge_pr");
    }

    async fn read_blob(
        &self,
        _snapshot_id: &str,
        _path: &str,
        _sha: &str,
    ) -> Result<GitHubBlob, KnowledgeToolError> {
        panic!("read backend should not be used for create_knowledge_pr");
    }
}

#[derive(Clone)]
struct FakePrRuntime {
    result: KnowledgeCreatePrResult,
}

#[async_trait]
impl KnowledgePrRuntime for FakePrRuntime {
    async fn create_pr(
        &self,
        args: &KnowledgeCreatePrArgs,
    ) -> Result<KnowledgeCreatePrResult, KnowledgeToolError> {
        assert_eq!(args.target_repo, "acme/docs");
        Ok(self.result.clone())
    }
}

#[derive(Clone)]
struct SymlinkSeedExecutor {
    outside_dir: Arc<std::path::PathBuf>,
    commit_called: Arc<AtomicBool>,
}

#[async_trait]
impl GitPrExecutor for SymlinkSeedExecutor {
    async fn ensure_auth(&self) -> Result<(), KnowledgeToolError> {
        Ok(())
    }

    async fn clone_repo(
        &self,
        _target_repo: &str,
        destination: &Path,
    ) -> Result<(), KnowledgeToolError> {
        std::fs::create_dir_all(destination).expect("destination should exist");
        #[cfg(unix)]
        std::os::unix::fs::symlink(self.outside_dir.as_path(), destination.join("docs"))
            .expect("symlink should create");
        Ok(())
    }

    async fn prepare_branch(
        &self,
        _repo_dir: &Path,
        _base_ref: &str,
        _branch: &str,
    ) -> Result<(), KnowledgeToolError> {
        Ok(())
    }

    async fn commit_and_push(
        &self,
        _repo_dir: &Path,
        _branch: &str,
        _commit_message: &str,
    ) -> Result<String, KnowledgeToolError> {
        self.commit_called.store(true, Ordering::SeqCst);
        Ok("unexpected".to_string())
    }

    async fn create_or_reuse_pr(
        &self,
        _repo_dir: &Path,
        _base_ref: &str,
        _branch: &str,
        _title: &str,
        _body: &str,
        _draft: bool,
    ) -> Result<GitPrOutcome, KnowledgeToolError> {
        panic!("create_or_reuse_pr should not be called after a write rejection");
    }
}

#[test]
fn validate_repo_relative_path_rejects_absolute_paths() {
    let err = validate_repo_relative_path("/etc/passwd").unwrap_err();
    assert!(err.to_string().contains("absolute"));
}

#[test]
fn validate_repo_relative_path_rejects_parent_dirs() {
    let err = validate_repo_relative_path("docs/../secrets.md").unwrap_err();
    assert!(err.to_string().contains(".."));
}

#[test]
fn validate_repo_relative_path_rejects_git_dir_paths() {
    let err = validate_repo_relative_path(".git/config").unwrap_err();
    assert!(err.to_string().contains(".git"));
}

#[test]
fn merge_manifest_creates_manifest_when_absent() {
    let merged = merge_manifest(None, &sample_patch()).unwrap();

    assert_eq!(merged.version, 1);
    let repo = merged.repo.as_ref().expect("repo metadata should exist");
    assert_eq!(repo.title.as_deref(), Some("Docs"));
    assert_eq!(repo.default_branch.as_deref(), None);
    assert_eq!(repo.include, vec!["docs", "api"]);
    assert_eq!(repo.exclude, vec!["tmp", "generated"]);
    assert_eq!(repo.entrypoints, vec!["README.md", "docs/guide.md"]);
    assert_eq!(merged.files.len(), 2);
    assert_eq!(merged.nodes.len(), 2);
}

#[test]
fn merge_manifest_upserts_files_and_nodes_by_path_and_id() {
    let merged = merge_manifest(Some(sample_existing_manifest()), &sample_patch()).unwrap();

    let guide = merged
        .files
        .iter()
        .find(|file| file.path == "docs/guide.md")
        .expect("guide file should exist");
    assert_eq!(guide.title.as_deref(), Some("Guide"));
    assert_eq!(guide.summary.as_deref(), Some("Updated summary"));
    assert_eq!(guide.tags, vec!["docs"]);
    assert_eq!(guide.aliases, vec!["guide", "start"]);

    let api = merged
        .files
        .iter()
        .find(|file| file.path == "docs/api.md")
        .expect("api file should exist");
    assert_eq!(api.title.as_deref(), Some("API"));
    assert_eq!(api.tags, vec!["api"]);

    let intro = merged
        .nodes
        .iter()
        .find(|node| node.id == "docs/guide#intro")
        .expect("intro node should exist");
    assert_eq!(intro.title.as_deref(), Some("Intro"));
    assert_eq!(intro.summary.as_deref(), Some("Updated node"));
    assert_eq!(intro.tags, vec!["docs"]);
    assert_eq!(intro.aliases, vec!["intro"]);

    let overview = merged
        .nodes
        .iter()
        .find(|node| node.id == "docs/api#overview")
        .expect("overview node should exist");
    assert_eq!(overview.source.path, "docs/api.md");
}

#[test]
fn merge_manifest_preserves_omitted_fields_for_existing_entries() {
    let patch = KnowledgeManifestPatch {
        path: Some(".knowledge/repo.json".to_string()),
        repo: None,
        files: Some(vec![KnowledgeManifestFilePatch {
            path: "docs/guide.md".to_string(),
            title: None,
            summary: Some("Updated summary".to_string()),
            tags: None,
            aliases: None,
        }]),
        nodes: Some(vec![KnowledgeManifestNodePatch {
            id: "docs/guide#intro".to_string(),
            source: KnowledgeManifestNodeSourcePatch {
                path: "docs/guide.md".to_string(),
                heading: Some("Intro".to_string()),
            },
            title: None,
            summary: Some("Updated node".to_string()),
            tags: None,
            aliases: None,
            relations: None,
        }]),
    };

    let merged = merge_manifest(Some(sample_existing_manifest()), &patch).unwrap();

    let guide = merged
        .files
        .iter()
        .find(|file| file.path == "docs/guide.md")
        .expect("guide file should exist");
    assert_eq!(guide.title.as_deref(), Some("Old guide"));
    assert_eq!(guide.summary.as_deref(), Some("Updated summary"));
    assert_eq!(guide.tags, vec!["legacy"]);
    assert_eq!(guide.aliases, vec!["guide"]);

    let intro = merged
        .nodes
        .iter()
        .find(|node| node.id == "docs/guide#intro")
        .expect("intro node should exist");
    assert_eq!(intro.title.as_deref(), Some("Old intro"));
    assert_eq!(intro.summary.as_deref(), Some("Updated node"));
    assert_eq!(intro.tags, vec!["legacy"]);
    assert_eq!(intro.aliases, vec!["intro"]);
    assert_eq!(intro.relations.len(), 1);
    assert_eq!(intro.relations[0].target, "docs/api#intro");
}

#[test]
fn serialize_manifest_is_stable_and_pretty() {
    let merged = merge_manifest(Some(sample_existing_manifest()), &sample_patch()).unwrap();
    let serialized = serialize_manifest(&merged).unwrap();

    assert_eq!(
        serialized,
        r#"{
  "version": 1,
  "repo": {
    "title": "Docs",
    "default_branch": "main",
    "include": [
      "docs",
      "api"
    ],
    "exclude": [
      "tmp",
      "generated"
    ],
    "entrypoints": [
      "README.md",
      "docs/guide.md"
    ]
  },
  "files": [
    {
      "path": "docs/guide.md",
      "title": "Guide",
      "summary": "Updated summary",
      "tags": [
        "docs"
      ],
      "aliases": [
        "guide",
        "start"
      ]
    },
    {
      "path": "docs/api.md",
      "title": "API",
      "tags": [
        "api"
      ],
      "aliases": [
        "reference"
      ]
    }
  ],
  "nodes": [
    {
      "id": "docs/guide#intro",
      "source": {
        "path": "docs/guide.md",
        "heading": "Intro"
      },
      "title": "Intro",
      "summary": "Updated node",
      "tags": [
        "docs"
      ],
      "aliases": [
        "intro"
      ],
      "relations": [
        {
          "type": "related",
          "target": "docs/api#intro"
        }
      ]
    },
    {
      "id": "docs/api#overview",
      "source": {
        "path": "docs/api.md",
        "heading": "Overview"
      },
      "title": "Overview",
      "tags": [
        "api"
      ],
      "aliases": [
        "reference"
      ],
      "relations": []
    }
  ]
}"#
    );
}

#[tokio::test]
async fn dispatch_create_knowledge_pr_returns_result_payload() {
    let runtime = DefaultKnowledgeRuntime::new_for_test_with_pr_runtime(
        NoopKnowledgeBackend,
        FakePrRuntime {
            result: KnowledgeCreatePrResult {
                target_repo: "acme/docs".to_string(),
                base_ref: "main".to_string(),
                branch: "codex/knowledge-bootstrap".to_string(),
                commit_sha: "abc123".to_string(),
                pr_url: "https://example.com/pr/42".to_string(),
                manifest_path: ".knowledge/repo.json".to_string(),
                changed_files: vec![
                    "docs/knowledge/README.md".to_string(),
                    ".knowledge/repo.json".to_string(),
                ],
                created_files: vec![
                    "docs/knowledge/README.md".to_string(),
                    ".knowledge/repo.json".to_string(),
                ],
                updated_files: Vec::new(),
                summary: "Opened draft PR for acme/docs with 2 changed files".to_string(),
            },
        },
    );
    let tool = KnowledgeTool::new_for_test(runtime);

    let result = tool
        .execute(
            serde_json::json!({
                "action": "create_knowledge_pr",
                "target_repo": "acme/docs",
                "base_ref": "main",
                "branch": "codex/knowledge-bootstrap",
                "pr_title": "Bootstrap knowledge docs",
                "pr_body": "Adds knowledge docs and manifest.",
                "draft": true,
                "files": [
                    {
                        "path": "docs/knowledge/README.md",
                        "content": "# Knowledge\n"
                    }
                ],
                "manifest": {
                    "path": ".knowledge/repo.json",
                    "files": []
                }
            }),
            make_ctx(),
        )
        .await
        .unwrap();

    assert_eq!(
        result,
        serde_json::json!({
            "target_repo": "acme/docs",
            "base_ref": "main",
            "branch": "codex/knowledge-bootstrap",
            "commit_sha": "abc123",
            "pr_url": "https://example.com/pr/42",
            "manifest_path": ".knowledge/repo.json",
            "changed_files": ["docs/knowledge/README.md", ".knowledge/repo.json"],
            "created_files": ["docs/knowledge/README.md", ".knowledge/repo.json"],
            "updated_files": [],
            "summary": "Opened draft PR for acme/docs with 2 changed files"
        })
    );
}

#[cfg(unix)]
#[tokio::test]
async fn executor_rejects_writes_through_symlinked_parents() {
    let temp = tempdir().expect("tempdir should create");
    let outside_dir = temp.path().join("outside");
    std::fs::create_dir_all(&outside_dir).expect("outside dir should exist");

    let commit_called = Arc::new(AtomicBool::new(false));
    let service = KnowledgePrService::new_with_executor(SymlinkSeedExecutor {
        outside_dir: Arc::new(outside_dir.clone()),
        commit_called: commit_called.clone(),
    });

    let err = service
        .create_pr(&KnowledgeCreatePrArgs {
            target_repo: "acme/docs".to_string(),
            base_ref: Some("main".to_string()),
            branch: Some("codex/knowledge-bootstrap".to_string()),
            pr_title: "Bootstrap knowledge docs".to_string(),
            pr_body: "Adds knowledge docs.".to_string(),
            draft: Some(true),
            files: vec![argus_tool::knowledge::KnowledgeFileWrite {
                path: "docs/escape.md".to_string(),
                content: "escaped\n".to_string(),
            }],
            manifest: None,
        })
        .await
        .unwrap_err();

    assert!(err.to_string().contains("symlink"));
    assert!(!outside_dir.join("escape.md").exists());
    assert!(!commit_called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn executor_successful_flow_writes_files_and_opens_pr() {
    let executor = FakeGitPrExecutor::with_state(FakeExecutorState {
        seed_files: HashMap::from([(
            "knowledge.json".to_string(),
            serde_json::json!({
                "version": 1,
                "repo": {
                    "title": "Existing docs",
                    "default_branch": "main",
                    "include": ["docs"],
                    "exclude": [],
                    "entrypoints": ["README.md"]
                },
                "files": [],
                "nodes": []
            })
            .to_string(),
        )]),
        created_pr_url: Some("https://example.com/pr/42".to_string()),
        commit_sha: "abc123".to_string(),
        ..FakeExecutorState::default()
    });
    let state = executor.state.clone();
    let service = KnowledgePrService::new_with_executor(executor);

    let result = service.create_pr(&sample_create_pr_args()).await.unwrap();

    assert_eq!(result.commit_sha, "abc123");
    assert_eq!(result.pr_url, "https://example.com/pr/42");
    assert_eq!(result.manifest_path, ".knowledge/repo.json");
    assert!(
        result
            .changed_files
            .contains(&"docs/knowledge/README.md".to_string())
    );
    assert!(
        result
            .changed_files
            .contains(&".knowledge/repo.json".to_string())
    );

    let state = state.lock().expect("state should lock");
    assert_eq!(
        state.calls,
        vec![
            "ensure_auth",
            "clone_repo",
            "prepare_branch:main:codex/knowledge-bootstrap",
            "commit_and_push:codex/knowledge-bootstrap",
            "create_or_reuse_pr:main:codex/knowledge-bootstrap:true",
        ]
    );
    assert_eq!(
        state.captured_files.get("docs/knowledge/README.md"),
        Some(&"# Knowledge\n".to_string())
    );
    assert!(
        state
            .captured_files
            .get(".knowledge/repo.json")
            .expect("manifest should be written")
            .contains("\"title\": \"Docs\"")
    );
}

#[tokio::test]
async fn executor_auth_failure_stops_before_clone() {
    let executor = FakeGitPrExecutor::with_state(FakeExecutorState {
        auth_error: Some("gh auth status failed".to_string()),
        ..FakeExecutorState::default()
    });
    let state = executor.state.clone();
    let service = KnowledgePrService::new_with_executor(executor);

    let err = service
        .create_pr(&sample_create_pr_args())
        .await
        .unwrap_err();

    assert!(err.to_string().contains("gh auth status failed"));
    let state = state.lock().expect("state should lock");
    assert_eq!(state.calls, vec!["ensure_auth"]);
}

#[tokio::test]
async fn executor_pr_creation_failure_happens_after_push() {
    let executor = FakeGitPrExecutor::with_state(FakeExecutorState {
        pr_error: Some("gh pr create failed".to_string()),
        commit_sha: "abc123".to_string(),
        ..FakeExecutorState::default()
    });
    let state = executor.state.clone();
    let service = KnowledgePrService::new_with_executor(executor);

    let err = service
        .create_pr(&sample_create_pr_args())
        .await
        .unwrap_err();

    assert!(err.to_string().contains("gh pr create failed"));
    let state = state.lock().expect("state should lock");
    assert_eq!(
        state.calls,
        vec![
            "ensure_auth",
            "clone_repo",
            "prepare_branch:main:codex/knowledge-bootstrap",
            "commit_and_push:codex/knowledge-bootstrap",
            "create_or_reuse_pr:main:codex/knowledge-bootstrap:true",
        ]
    );
}

#[tokio::test]
async fn executor_reuses_existing_pr() {
    let executor = FakeGitPrExecutor::with_state(FakeExecutorState {
        existing_pr_url: Some("https://example.com/pr/existing".to_string()),
        commit_sha: "def456".to_string(),
        ..FakeExecutorState::default()
    });
    let service = KnowledgePrService::new_with_executor(executor);

    let result = service.create_pr(&sample_create_pr_args()).await.unwrap();

    assert_eq!(result.pr_url, "https://example.com/pr/existing");
    assert!(result.summary.contains("Updated existing PR"));
}

#[tokio::test]
async fn cli_executor_reuses_existing_remote_branch_history() {
    let temp = init_origin_with_main();
    let origin = temp.path().join("origin.git");
    let origin_str = origin.to_string_lossy().to_string();
    let executor = CliGitPrExecutor::new();
    let branch = "codex/knowledge-pr-update";

    let first = temp.path().join("first");
    let first_str = first.to_string_lossy().to_string();
    run_git(temp.path(), &["clone", &origin_str, &first_str]);
    configure_git_identity(&first);
    executor
        .prepare_branch(&first, "main", branch)
        .await
        .unwrap();
    std::fs::write(first.join("docs.txt"), "first\n").expect("file should write");
    let first_sha = executor
        .commit_and_push(&first, branch, "first change")
        .await
        .unwrap();

    let second = temp.path().join("second");
    let second_str = second.to_string_lossy().to_string();
    run_git(temp.path(), &["clone", &origin_str, &second_str]);
    configure_git_identity(&second);
    executor
        .prepare_branch(&second, "main", branch)
        .await
        .unwrap();
    std::fs::write(second.join("docs.txt"), "second\n").expect("file should write");
    let second_sha = executor
        .commit_and_push(&second, branch, "second change")
        .await
        .unwrap();

    run_git(
        &second,
        &["merge-base", "--is-ancestor", &first_sha, &second_sha],
    );
    let remote_head = git_git_dir_output(&origin, &["rev-parse", &format!("refs/heads/{branch}")]);
    assert_eq!(remote_head, second_sha);
    let remote_content = git_output(&second, &["show", &format!("origin/{branch}:docs.txt")]);
    assert_eq!(remote_content, "second");
}

#[tokio::test]
async fn cli_executor_commit_and_push_is_noop_when_branch_is_already_up_to_date() {
    let temp = init_origin_with_main();
    let origin = temp.path().join("origin.git");
    let origin_str = origin.to_string_lossy().to_string();
    let executor = CliGitPrExecutor::new();
    let branch = "codex/knowledge-pr-update";

    let first = temp.path().join("first");
    let first_str = first.to_string_lossy().to_string();
    run_git(temp.path(), &["clone", &origin_str, &first_str]);
    configure_git_identity(&first);
    executor
        .prepare_branch(&first, "main", branch)
        .await
        .unwrap();
    std::fs::write(first.join("docs.txt"), "first\n").expect("file should write");
    let first_sha = executor
        .commit_and_push(&first, branch, "first change")
        .await
        .unwrap();

    let second = temp.path().join("second");
    let second_str = second.to_string_lossy().to_string();
    run_git(temp.path(), &["clone", &origin_str, &second_str]);
    configure_git_identity(&second);
    executor
        .prepare_branch(&second, "main", branch)
        .await
        .unwrap();
    let second_sha = executor
        .commit_and_push(&second, branch, "no-op change")
        .await
        .unwrap();

    assert_eq!(second_sha, first_sha);
    let remote_head = git_git_dir_output(&origin, &["rev-parse", &format!("refs/heads/{branch}")]);
    assert_eq!(remote_head, first_sha);
}
