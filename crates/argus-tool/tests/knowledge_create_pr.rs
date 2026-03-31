use argus_tool::knowledge::{
    KnowledgeManifestFilePatch, KnowledgeManifestNodePatch, KnowledgeManifestNodeSourcePatch,
    KnowledgeManifestPatch, KnowledgeManifestRepoPatch, RepositoryManifest,
    merge_manifest, serialize_manifest, validate_repo_relative_path,
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
