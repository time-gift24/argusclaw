use std::path::PathBuf;
use std::sync::Arc;

use argus_protocol::ToolExecutionContext;
use argus_protocol::ids::ThreadId;
use argus_tool::{ChromeTool, ToolManager};
use tokio::sync::broadcast;

fn chrome_binary_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    for env_name in ["ARGUS_CHROME_BINARY", "CHROME_BINARY"] {
        if let Some(path) = std::env::var_os(env_name) {
            candidates.push(PathBuf::from(path));
        }
    }

    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ));
        candidates.push(PathBuf::from(
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ));
    } else if cfg!(target_os = "linux") {
        candidates.push(PathBuf::from("/usr/bin/google-chrome"));
        candidates.push(PathBuf::from("/usr/bin/google-chrome-stable"));
        candidates.push(PathBuf::from("/usr/bin/chromium"));
        candidates.push(PathBuf::from("/usr/bin/chromium-browser"));
    } else if cfg!(target_os = "windows") {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let base = PathBuf::from(local_app_data).join("Google").join("Chrome");
            candidates.push(base.join("Application").join("chrome.exe"));
        }
        if let Some(program_files) = std::env::var_os("PROGRAMFILES") {
            candidates.push(
                PathBuf::from(program_files)
                    .join("Google")
                    .join("Chrome")
                    .join("Application")
                    .join("chrome.exe"),
            );
        }
        if let Some(program_files_x86) = std::env::var_os("PROGRAMFILES(X86)") {
            candidates.push(
                PathBuf::from(program_files_x86)
                    .join("Google")
                    .join("Chrome")
                    .join("Application")
                    .join("chrome.exe"),
            );
        }
    }

    candidates
}

fn local_chrome_binary() -> Option<PathBuf> {
    chrome_binary_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file())
}

fn make_ctx() -> Arc<ToolExecutionContext> {
    let (pipe_tx, _) = broadcast::channel(16);
    Arc::new(ToolExecutionContext {
        thread_id: ThreadId::new(),
        agent_id: None,
        pipe_tx,
    })
}

#[tokio::test]
async fn smoke_test_skips_without_env_flag() {
    if std::env::var("ARGUS_CHROME_SMOKE").as_deref() != Ok("1") {
        return;
    }

    if local_chrome_binary().is_none() {
        return;
    }

    let manager = ToolManager::new();
    manager.register(Arc::new(ChromeTool::new()));

    manager
        .execute(
            "chrome",
            serde_json::json!({ "action": "install" }),
            make_ctx(),
        )
        .await
        .expect("chrome install should succeed");

    let open = manager
        .execute(
            "chrome",
            serde_json::json!({
                "action": "open",
                "url": "https://example.com"
            }),
            make_ctx(),
        )
        .await
        .expect("chrome open should succeed");

    let session_id = open["session_id"]
        .as_str()
        .expect("open should return a session id")
        .to_owned();

    let extract = manager
        .execute(
            "chrome",
            serde_json::json!({
                "action": "extract_text",
                "session_id": session_id,
                "selector": "body"
            }),
            make_ctx(),
        )
        .await
        .expect("chrome extract_text should succeed");

    let content = extract["content"]
        .as_str()
        .expect("extract_text should return text content");
    assert!(content.contains("Example Domain"));
}
