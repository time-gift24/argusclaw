use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;

use super::cache::SnapshotCache;
use super::error::KnowledgeToolError;
use super::manifest::RepositoryManifest;
use super::markdown::parse_markdown_sections;
use super::models::{
    ContentPage, ExploreTreeEntry, ExploreTreeResult, GitHubBlob, GitHubTree, GitHubTreeEntryKind,
    KnowledgeNode, KnowledgeNodeKind, KnowledgeSource,
};

#[async_trait]
pub trait KnowledgeBackend: Send + Sync {
    async fn read_tree(&self, snapshot_id: &str) -> Result<GitHubTree, KnowledgeToolError>;

    async fn read_manifest(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<RepositoryManifest>, KnowledgeToolError>;

    async fn read_blob(
        &self,
        snapshot_id: &str,
        path: &str,
        sha: &str,
    ) -> Result<GitHubBlob, KnowledgeToolError>;
}

pub struct KnowledgeIndexer<B> {
    backend: Arc<B>,
    snapshots: DashMap<String, Arc<SnapshotCache>>,
}

impl<B> KnowledgeIndexer<B>
where
    B: KnowledgeBackend + 'static,
{
    pub fn new(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            snapshots: DashMap::new(),
        }
    }

    pub async fn explore_tree(
        &self,
        snapshot_id: &str,
        path: &str,
        depth: usize,
    ) -> Result<ExploreTreeResult, KnowledgeToolError> {
        let cache = self.ensure_snapshot(snapshot_id).await?;
        let normalized = normalize_path(path);

        let mut aggregates = BTreeMap::<String, TreeEntryAggregate>::new();

        for entry in &cache.tree.entries {
            let Some(relative) = relative_path(&entry.path, &normalized) else {
                continue;
            };

            let segments = relative
                .split('/')
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>();
            if segments.is_empty() {
                continue;
            }

            let max_level = depth.min(segments.len());
            for level in 1..=max_level {
                let path_segments = segments[..level].join("/");
                let full_path = join_scope_and_path(&normalized, &path_segments);
                let is_last_level = level == segments.len();
                let is_directory = !is_last_level || entry.kind == GitHubTreeEntryKind::Tree;

                let aggregate =
                    aggregates
                        .entry(full_path.clone())
                        .or_insert_with(|| TreeEntryAggregate {
                            title: file_title(&full_path),
                            summary_hint: None,
                            is_directory: false,
                            direct_children: BTreeSet::new(),
                        });

                aggregate.is_directory |= is_directory;

                if level < segments.len() {
                    aggregate
                        .direct_children
                        .insert(segments[level].to_string());
                } else if entry.kind == GitHubTreeEntryKind::Blob {
                    aggregate.summary_hint = cache
                        .manifest
                        .as_ref()
                        .and_then(|manifest| manifest.file_override(&entry.path))
                        .and_then(|file| file.summary.clone());
                }
            }
        }

        let entries = aggregates
            .into_iter()
            .map(|(full_path, aggregate)| ExploreTreeEntry {
                path: format!("/{full_path}"),
                title: aggregate.title,
                child_count: if aggregate.is_directory {
                    aggregate.direct_children.len()
                } else {
                    0
                },
                summary_hint: aggregate.summary_hint,
            })
            .collect();

        Ok(ExploreTreeResult {
            entries,
            truncated: false,
        })
    }

    pub async fn search_nodes(
        &self,
        snapshot_id: &str,
        query: &str,
        scope_path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<KnowledgeNode>, KnowledgeToolError> {
        let cache = self.ensure_snapshot(snapshot_id).await?;
        self.ensure_sections_loaded(snapshot_id, cache.as_ref(), scope_path)
            .await?;

        let mut nodes: Vec<KnowledgeNode> = cache
            .nodes
            .iter()
            .map(|entry| entry.value().clone())
            .filter(|node| matches_scope(node, scope_path))
            .filter(|node| matches_query(node, query))
            .collect();

        nodes.sort_by(|left, right| left.id.cmp(&right.id));
        nodes.truncate(limit);
        Ok(nodes)
    }

    pub async fn get_node(
        &self,
        snapshot_id: &str,
        node_id: &str,
    ) -> Result<KnowledgeNode, KnowledgeToolError> {
        let cache = self.ensure_snapshot(snapshot_id).await?;
        self.ensure_sections_loaded(snapshot_id, cache.as_ref(), None)
            .await?;
        cache
            .nodes
            .get(node_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| KnowledgeToolError::NotFound(node_id.to_string()))
    }

    pub async fn get_content(
        &self,
        snapshot_id: &str,
        node_id: &str,
        max_chars: Option<usize>,
        cursor: Option<&str>,
    ) -> Result<ContentPage, KnowledgeToolError> {
        let cache = self.ensure_snapshot(snapshot_id).await?;
        self.ensure_sections_loaded(snapshot_id, cache.as_ref(), None)
            .await?;
        let node = cache
            .nodes
            .get(node_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| KnowledgeToolError::NotFound(node_id.to_string()))?;

        let blob_sha = node.source.blob_sha.clone();
        let blob = self
            .backend
            .read_blob(snapshot_id, &node.path, &blob_sha)
            .await?;
        let excerpt = extract_excerpt(&blob.text, node.source.start_line, node.source.end_line);
        let offset = cursor
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let limit = max_chars.unwrap_or(2_400);

        let content = excerpt.chars().skip(offset).take(limit).collect::<String>();
        let total_chars = excerpt.chars().count();
        let consumed = offset + content.chars().count();
        let truncated = consumed < total_chars;

        Ok(ContentPage {
            content,
            truncated,
            next_cursor: truncated.then(|| consumed.to_string()),
            source: node.source,
        })
    }

    pub async fn get_neighbors(
        &self,
        snapshot_id: &str,
        node_id: &str,
    ) -> Result<Vec<KnowledgeNode>, KnowledgeToolError> {
        let cache = self.ensure_snapshot(snapshot_id).await?;
        self.ensure_sections_loaded(snapshot_id, cache.as_ref(), None)
            .await?;
        let node = cache
            .nodes
            .get(node_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| KnowledgeToolError::NotFound(node_id.to_string()))?;

        let mut neighbors = Vec::new();
        for child_id in &node.children {
            if let Some(child) = cache.nodes.get(child_id) {
                neighbors.push(child.value().clone());
            }
        }

        for relation in &node.relations {
            if let Some(related) = cache.nodes.get(&relation.target) {
                neighbors.push(related.value().clone());
            }
        }

        Ok(neighbors)
    }

    async fn ensure_snapshot(
        &self,
        snapshot_id: &str,
    ) -> Result<Arc<SnapshotCache>, KnowledgeToolError> {
        if let Some(existing) = self.snapshots.get(snapshot_id) {
            return Ok(existing.value().clone());
        }

        let tree = self.backend.read_tree(snapshot_id).await?;
        let manifest = self.backend.read_manifest(snapshot_id).await?;
        let cache = Arc::new(SnapshotCache::new(tree, manifest));
        self.snapshots
            .insert(snapshot_id.to_string(), cache.clone());
        Ok(cache)
    }

    async fn ensure_sections_loaded(
        &self,
        snapshot_id: &str,
        cache: &SnapshotCache,
        scope_path: Option<&str>,
    ) -> Result<(), KnowledgeToolError> {
        let normalized_scope = scope_path.map(normalize_path);

        for entry in &cache.tree.entries {
            if entry.kind != GitHubTreeEntryKind::Blob || !entry.path.ends_with(".md") {
                continue;
            }

            if let Some(scope) = &normalized_scope
                && !entry.path.starts_with(scope)
            {
                continue;
            }

            let file_id = entry.path.clone();
            if cache.nodes.contains_key(&file_id) {
                continue;
            }

            let blob = self
                .backend
                .read_blob(snapshot_id, &entry.path, &entry.sha)
                .await?;
            let file_override = cache
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.file_override(&entry.path));
            let file_node = KnowledgeNode {
                id: file_id.clone(),
                kind: KnowledgeNodeKind::File,
                title: file_override
                    .and_then(|override_| override_.title.clone())
                    .unwrap_or_else(|| file_title(&entry.path)),
                path: entry.path.clone(),
                anchor: None,
                summary: file_override.and_then(|override_| override_.summary.clone()),
                aliases: file_override
                    .map(|override_| override_.aliases.clone())
                    .unwrap_or_default(),
                tags: file_override
                    .map(|override_| override_.tags.clone())
                    .unwrap_or_default(),
                relations: Vec::new(),
                children: Vec::new(),
                source: KnowledgeSource {
                    path: entry.path.clone(),
                    blob_sha: entry.sha.clone(),
                    start_line: 1,
                    end_line: blob.text.lines().count().max(1),
                },
            };

            cache.nodes.insert(file_id.clone(), file_node);

            let sections = parse_markdown_sections(&entry.path, &blob.text);
            for section in sections {
                let generated_id = format!("{}#{}", entry.path, section.anchor);
                let node_id = cache
                    .manifest
                    .as_ref()
                    .map(|manifest| {
                        manifest.resolve_section_id(&entry.path, &section.title, &generated_id)
                    })
                    .unwrap_or_else(|| generated_id.clone());

                let node_override = cache.manifest.as_ref().and_then(|manifest| {
                    manifest.nodes.iter().find(|node| {
                        node.id == node_id
                            || node.source.path == entry.path
                                && node.source.heading.as_deref() == Some(section.title.as_str())
                    })
                });

                cache.nodes.insert(
                    node_id.clone(),
                    KnowledgeNode {
                        id: node_id.clone(),
                        kind: KnowledgeNodeKind::Section,
                        title: node_override
                            .and_then(|override_| override_.title.clone())
                            .unwrap_or_else(|| section.title.clone()),
                        path: entry.path.clone(),
                        anchor: Some(section.anchor.clone()),
                        summary: node_override.and_then(|override_| override_.summary.clone()),
                        aliases: node_override
                            .map(|override_| override_.aliases.clone())
                            .unwrap_or_default(),
                        tags: node_override
                            .map(|override_| override_.tags.clone())
                            .unwrap_or_default(),
                        relations: node_override
                            .map(|override_| override_.relations.clone())
                            .unwrap_or_default(),
                        children: Vec::new(),
                        source: KnowledgeSource {
                            path: entry.path.clone(),
                            blob_sha: entry.sha.clone(),
                            start_line: section.start_line,
                            end_line: section.end_line,
                        },
                    },
                );

                if let Some(mut file_node) = cache.nodes.get_mut(&file_id) {
                    file_node.children.push(node_id);
                }
            }
        }

        Ok(())
    }
}

fn normalize_path(path: &str) -> String {
    path.trim_matches('/').to_string()
}

fn join_scope_and_path(scope: &str, relative: &str) -> String {
    if scope.is_empty() {
        relative.to_string()
    } else {
        format!("{scope}/{relative}")
    }
}

fn relative_path<'a>(path: &'a str, scope: &str) -> Option<&'a str> {
    if scope.is_empty() {
        return Some(path);
    }

    let prefix = format!("{scope}/");
    path.strip_prefix(&prefix)
}

fn file_title(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(path)
        .to_string()
}

fn matches_scope(node: &KnowledgeNode, scope_path: Option<&str>) -> bool {
    scope_path
        .map(normalize_path)
        .map(|scope| node.path.starts_with(&scope))
        .unwrap_or(true)
}

fn matches_query(node: &KnowledgeNode, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {} {}",
        node.title,
        node.path,
        node.summary.clone().unwrap_or_default(),
        node.aliases.join(" "),
        node.tags.join(" ")
    )
    .to_lowercase();

    query
        .split_whitespace()
        .map(str::to_lowercase)
        .any(|token| haystack.contains(&token))
}

fn extract_excerpt(content: &str, start_line: usize, end_line: usize) -> String {
    content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line_number = index + 1;
            (line_number >= start_line && line_number <= end_line).then_some(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct TreeEntryAggregate {
    title: String,
    summary_hint: Option<String>,
    is_directory: bool,
    direct_children: BTreeSet<String>,
}
