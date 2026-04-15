use std::sync::Arc;
use std::time::Duration;

use argus_protocol::ToolExecutionContext;
use argus_protocol::ids::ThreadId;
use argus_tool::{ChromeTool, ToolManager};
use clap::{Parser, Subcommand};
use serde_json::json;
use tokio::sync::broadcast;

#[derive(Debug, Parser)]
#[command(
    name = "argus-chrome-cli",
    version,
    about = "Manual smoke test for Chrome tool"
)]
struct Cli {
    /// Deprecated no-op; chrome-cli always uses interactive Chrome actions.
    #[arg(long, global = true, hide = true)]
    _interactive: bool,

    /// Pretty-print JSON output
    #[arg(long, global = true, default_value_t = false)]
    pretty: bool,

    /// Keep the browser session open for the specified milliseconds after `navigate`
    #[arg(long, global = true, default_value_t = 0)]
    hold_ms: u64,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Install matching ChromeDriver for current Chrome version
    Install,
    /// Navigate the shared browser session to a URL
    Navigate {
        /// HTTP or HTTPS URL to open
        #[arg(long)]
        url: String,
    },
    /// Wait up to the specified milliseconds (default: 1ms, capped at 1000ms)
    Wait {
        /// Optional milliseconds to wait
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// Extract text from the page body or a CSS selector
    ExtractText {
        /// CSS selector (optional, defaults to body)
        #[arg(long)]
        selector: Option<String>,
    },
    /// Click an element
    Click {
        /// CSS selector
        #[arg(long)]
        selector: String,
    },
    /// Type text into an input element
    Type {
        /// CSS selector
        #[arg(long)]
        selector: String,
        /// Text to type
        #[arg(long)]
        text: String,
    },
    /// Get current page URL
    GetUrl,
    /// Get cookies from the active page
    GetCookies {
        /// Optional cookie domain filter
        #[arg(long)]
        domain: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = run(cli.command, cli.hold_ms).await;

    let output = if cli.pretty {
        serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
            String::from("{\"ok\":false,\"error\":\"failed to format output\"}")
        })
    } else {
        serde_json::to_string(&result).unwrap_or_else(|_| {
            String::from("{\"ok\":false,\"error\":\"failed to format output\"}")
        })
    };

    println!("{output}");
}

fn make_ctx() -> Arc<ToolExecutionContext> {
    let (pipe_tx, _) = broadcast::channel(16);
    Arc::new(ToolExecutionContext {
        thread_id: ThreadId::new(),
        agent_id: None,
        pipe_tx,
    })
}

fn payload_for_command(command: &Command) -> (&'static str, serde_json::Value) {
    match command {
        Command::Install => ("install", json!({ "action": "install" })),
        Command::Navigate { url } => ("navigate", json!({ "action": "navigate", "url": url })),
        Command::Wait { timeout_ms } => (
            "wait",
            json!({
                "action": "wait",
                "timeout_ms": timeout_ms,
            }),
        ),
        Command::ExtractText { selector } => (
            "extract_text",
            json!({
                "action": "extract_text",
                "selector": selector,
            }),
        ),
        Command::Click { selector } => (
            "click",
            json!({
                "action": "click",
                "selector": selector,
            }),
        ),
        Command::Type { selector, text } => (
            "type",
            json!({
                "action": "type",
                "selector": selector,
                "text": text,
            }),
        ),
        Command::GetUrl => ("get_url", json!({ "action": "get_url" })),
        Command::GetCookies { domain } => (
            "get_cookies",
            json!({
                "action": "get_cookies",
                "domain": domain,
            }),
        ),
    }
}

async fn run(command: Command, hold_ms: u64) -> serde_json::Value {
    let tool = ChromeTool::new_interactive();

    let manager = ToolManager::new();
    manager.register(Arc::new(tool));

    let (action, request) = payload_for_command(&command);
    let result = manager.execute("chrome", request, make_ctx()).await;

    if hold_ms > 0 && matches!(command, Command::Navigate { .. }) {
        tokio::time::sleep(Duration::from_millis(hold_ms)).await;
    }

    match result {
        Ok(result) => json!({
            "ok": true,
            "result": result,
        }),
        Err(error) => json!({
            "ok": false,
            "action": action,
            "error": error.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn chrome_cli_allows_interactive_actions_by_default() {
        let result = run(
            Command::Click {
                selector: "#login".to_string(),
            },
            0,
        )
        .await;

        assert_eq!(result["ok"], false);
        let error = result["error"].as_str().unwrap_or_default();
        assert!(
            error.contains("shared browser session is unavailable"),
            "unexpected error: {error}"
        );
        assert!(
            !error.contains("not authorized") && !error.contains("not allowed"),
            "interactive action should not be blocked by readonly policy: {error}"
        );
    }
}
