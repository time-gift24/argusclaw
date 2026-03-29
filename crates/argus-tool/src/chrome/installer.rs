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
