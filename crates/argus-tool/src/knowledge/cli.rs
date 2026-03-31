use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;

use super::error::KnowledgeToolError;

pub struct CliOutput {
    pub stdout: String,
}

#[async_trait]
pub trait CliRunner: Send + Sync {
    async fn run(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
    ) -> Result<CliOutput, KnowledgeToolError>;
}

pub struct RealCliRunner;

#[async_trait]
impl CliRunner for RealCliRunner {
    async fn run(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
    ) -> Result<CliOutput, KnowledgeToolError> {
        let output = tokio::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| KnowledgeToolError::RequestFailed(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(KnowledgeToolError::RequestFailed(format!(
                "{program} {} failed: {stderr}",
                args.join(" ")
            )));
        }

        Ok(CliOutput {
            stdout,
        })
    }
}
