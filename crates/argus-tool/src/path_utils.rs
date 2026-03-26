//! Path validation utilities for tools that access the filesystem.

use std::path::{Path, PathBuf};

use crate::ToolError;

/// Normalize a path by resolving `.` and `..` components lexically (no filesystem access).
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                if components
                    .last()
                    .is_some_and(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    components.pop();
                }
            }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

/// Validate that a path is safe (no traversal attacks).
pub fn validate_path(path_str: &str, base_dir: Option<&Path>) -> Result<PathBuf, ToolError> {
    if !is_path_safe_minimal(path_str) {
        return Err(ToolError::NotAuthorized(format!(
            "Path contains forbidden characters or sequences: {}",
            path_str
        )));
    }

    let path = PathBuf::from(path_str);

    let resolved = if path.is_absolute() {
        path.canonicalize()
            .unwrap_or_else(|_| normalize_lexical(&path))
    } else if let Some(base) = base_dir {
        let joined = base.join(&path);
        joined
            .canonicalize()
            .unwrap_or_else(|_| normalize_lexical(&joined))
    } else {
        let joined = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&path);
        normalize_lexical(&joined)
    };

    if let Some(base) = base_dir {
        let base_canonical = base
            .canonicalize()
            .unwrap_or_else(|_| normalize_lexical(base));

        let check_path = if resolved.exists() {
            resolved.canonicalize().unwrap_or_else(|_| resolved.clone())
        } else {
            let mut ancestor = resolved.as_path();
            let mut tail_parts: Vec<&std::ffi::OsStr> = Vec::new();
            loop {
                if ancestor.exists() {
                    let canonical_ancestor = ancestor
                        .canonicalize()
                        .unwrap_or_else(|_| ancestor.to_path_buf());
                    let mut result = canonical_ancestor;
                    for part in tail_parts.into_iter().rev() {
                        result = result.join(part);
                    }
                    break result;
                }
                if let Some(name) = ancestor.file_name() {
                    tail_parts.push(name);
                }
                match ancestor.parent() {
                    Some(parent) if parent != ancestor => ancestor = parent,
                    _ => break resolved.clone(),
                }
            }
        };

        if !check_path.starts_with(&base_canonical) {
            return Err(ToolError::NotAuthorized(format!(
                "Path escapes sandbox: {}",
                path_str
            )));
        }
    }

    Ok(resolved)
}

fn is_path_safe_minimal(path: &str) -> bool {
    if path.contains('\0') {
        return false;
    }

    let lower = path.to_lowercase();
    if lower.contains("%2e") || lower.contains("%2f") || lower.contains("%5c") {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_normalize_lexical_basic() {
        assert_eq!(
            normalize_lexical(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            normalize_lexical(Path::new("/a/b/c/../../d")),
            PathBuf::from("/a/d")
        );
        assert_eq!(
            normalize_lexical(Path::new("/a/./b/./c")),
            PathBuf::from("/a/b/c")
        );
        assert_eq!(
            normalize_lexical(Path::new("/a/../../..")),
            PathBuf::from("/")
        );
    }

    #[test]
    fn test_validate_path_rejects_traversal() {
        let dir = tempdir().unwrap();
        assert!(validate_path("../../etc/passwd", Some(dir.path())).is_err());
    }

    #[test]
    fn test_validate_path_allows_nested() {
        let dir = tempdir().unwrap();
        assert!(validate_path("subdir/file.txt", Some(dir.path())).is_ok());
    }
}
