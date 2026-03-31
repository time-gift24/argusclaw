use std::path::{Component, Path};

use super::error::KnowledgeToolError;
use super::manifest::{FileOverride, NodeOverride, RepositoryManifest, RepositoryManifestMeta};
use super::models::{
    KnowledgeManifestFilePatch, KnowledgeManifestNodePatch, KnowledgeManifestPatch,
    KnowledgeManifestRepoPatch,
};

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
    let replacement = FileOverride {
        path: patch.path.clone(),
        title: patch.title.clone(),
        summary: patch.summary.clone(),
        tags: patch.tags.clone().unwrap_or_default(),
        aliases: patch.aliases.clone().unwrap_or_default(),
    };

    if let Some(index) = files.iter().position(|file| file.path == replacement.path) {
        files[index] = replacement;
    } else {
        files.push(replacement);
    }
}

fn upsert_node(nodes: &mut Vec<NodeOverride>, patch: &KnowledgeManifestNodePatch) {
    let replacement = NodeOverride {
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
    };

    if let Some(index) = nodes.iter().position(|node| node.id == replacement.id) {
        nodes[index] = replacement;
    } else {
        nodes.push(replacement);
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
    write_array(output, relations, indent, |output, relation, item_indent| {
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
    })
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
