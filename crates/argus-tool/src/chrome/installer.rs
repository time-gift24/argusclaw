use std::path::{Path, PathBuf};

use super::error::ChromeToolError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromePaths {
    pub root: PathBuf,
    pub driver: PathBuf,
    pub patched: PathBuf,
    pub screenshots: PathBuf,
    pub tmp: PathBuf,
}

impl ChromePaths {
    #[must_use]
    pub fn from_home(home: &Path) -> Self {
        let root = home.join(".arguswing").join("chrome");
        let driver = root.join("driver");
        let patched = root.join("patched");
        let screenshots = root.join("screenshots");
        let tmp = root.join("tmp");

        Self {
            root,
            driver,
            patched,
            screenshots,
            tmp,
        }
    }

    pub fn ensure_directories(&self) -> Result<(), ChromeToolError> {
        create_directory(&self.root)?;
        create_directory(&self.driver)?;
        create_directory(&self.patched)?;
        create_directory(&self.screenshots)?;
        create_directory(&self.tmp)?;
        Ok(())
    }
}

fn create_directory(path: &Path) -> Result<(), ChromeToolError> {
    std::fs::create_dir_all(path).map_err(|e| ChromeToolError::DirectoryCreateFailed {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn ensure_directories_creates_expected_tree() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());

        paths.ensure_directories().unwrap();

        assert!(paths.root.is_dir());
        assert!(paths.driver.is_dir());
        assert!(paths.patched.is_dir());
        assert!(paths.screenshots.is_dir());
        assert!(paths.tmp.is_dir());
    }

    #[test]
    fn ensure_directories_returns_create_failed_error() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());

        std::fs::create_dir_all(paths.root.parent().unwrap()).unwrap();
        std::fs::write(&paths.root, b"file-blocking-directory").unwrap();

        let err = paths.ensure_directories().unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::DirectoryCreateFailed { path, .. } if path == paths.root
        ));
    }
}
