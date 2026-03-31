use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use argus_tool::knowledge::{
    GitPrExecutor, GitPrOutcome, KnowledgeCreatePrArgs, KnowledgeManifestFilePatch,
    KnowledgeManifestNodePatch, KnowledgeManifestNodeSourcePatch, KnowledgeManifestPatch,
    KnowledgeManifestRepoPatch, KnowledgePrService, RepositoryManifest, merge_manifest,
    serialize_manifest, validate_repo_relative_path,
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
    assert!(result
        .changed_files
        .contains(&"docs/knowledge/README.md".to_string()));
    assert!(result
        .changed_files
        .contains(&".knowledge/repo.json".to_string()));

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

    let err = service.create_pr(&sample_create_pr_args()).await.unwrap_err();

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

    let err = service.create_pr(&sample_create_pr_args()).await.unwrap_err();

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
