use std::sync::Arc;
use std::time::Duration;

use argus_protocol::ToolExecutionContext;
use argus_protocol::ids::ThreadId;
use argus_tool::{ChromeTool, ToolManager};
use clap::{Parser, Subcommand};
use serde_json::json;
use tokio::sync::{broadcast, mpsc};

#[derive(Debug, Parser)]
#[command(
    name = "argus-chrome-cli",
    version,
    about = "Manual smoke test for Chrome tool"
)]
struct Cli {
    /// Use interactive Chrome actions (click/type/get_url/get_cookies)
    #[arg(long, global = true)]
    interactive: bool,

    /// Pretty-print JSON output
    #[arg(long, global = true, default_value_t = false)]
    pretty: bool,

    /// Keep interactive session open for the specified milliseconds after `open`
    #[arg(long, global = true, default_value_t = 0)]
    hold_ms: u64,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Install matching ChromeDriver for current Chrome version
    Install,
    /// Open a URL in browser and create a session
    Open {
        /// HTTP or HTTPS URL to open
        #[arg(long)]
        url: String,
    },
    /// Wait up to the specified milliseconds (default: 1ms, capped at 1000ms)
    Wait {
        /// Session ID returned by open
        #[arg(long = "session-id")]
        session_id: String,
        /// Optional milliseconds to wait
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// Extract text from the page body or a CSS selector
    ExtractText {
        /// Session ID returned by open
        #[arg(long = "session-id")]
        session_id: String,
        /// CSS selector (optional, defaults to body)
        #[arg(long)]
        selector: Option<String>,
    },
    /// List all links on the page
    ListLinks {
        /// Session ID returned by open
        #[arg(long = "session-id")]
        session_id: String,
    },
    /// Get DOM text summary for the page
    GetDomSummary {
        /// Session ID returned by open
        #[arg(long = "session-id")]
        session_id: String,
    },
    /// Capture a screenshot and save to temporary path
    Screenshot {
        /// Session ID returned by open
        #[arg(long = "session-id")]
        session_id: String,
    },
    /// Click an element (interactive mode required)
    Click {
        /// Session ID returned by open
        #[arg(long = "session-id")]
        session_id: String,
        /// CSS selector
        #[arg(long)]
        selector: String,
    },
    /// Type text into an input element (interactive mode required)
    Type {
        /// Session ID returned by open
        #[arg(long = "session-id")]
        session_id: String,
        /// CSS selector
        #[arg(long)]
        selector: String,
        /// Text to type
        #[arg(long)]
        text: String,
    },
    /// Get current page URL (interactive mode required)
    GetUrl {
        /// Session ID returned by open
        #[arg(long = "session-id")]
        session_id: String,
    },
    /// Get cookies from the active page (interactive mode required)
    GetCookies {
        /// Session ID returned by open
        #[arg(long = "session-id")]
        session_id: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = run(cli.command, cli.interactive, cli.hold_ms).await;

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
    let (control_tx, _control_rx) = mpsc::unbounded_channel();
    Arc::new(ToolExecutionContext {
        thread_id: ThreadId::new(),
        pipe_tx,
        control_tx,
    })
}

fn payload_for_command(command: &Command) -> (&'static str, serde_json::Value) {
    match command {
        Command::Install => ("install", json!({ "action": "install" })),
        Command::Open { url } => ("open", json!({ "action": "open", "url": url })),
        Command::Wait {
            session_id,
            timeout_ms,
        } => (
            "wait",
            json!({
                "action": "wait",
                "session_id": session_id,
                "timeout_ms": timeout_ms,
            }),
        ),
        Command::ExtractText {
            session_id,
            selector,
        } => (
            "extract_text",
            json!({
                "action": "extract_text",
                "session_id": session_id,
                "selector": selector,
            }),
        ),
        Command::ListLinks { session_id } => (
            "list_links",
            json!({
                "action": "list_links",
                "session_id": session_id,
            }),
        ),
        Command::GetDomSummary { session_id } => (
            "get_dom_summary",
            json!({
                "action": "get_dom_summary",
                "session_id": session_id,
            }),
        ),
        Command::Screenshot { session_id } => (
            "screenshot",
            json!({
                "action": "screenshot",
                "session_id": session_id,
            }),
        ),
        Command::Click {
            session_id,
            selector,
        } => (
            "click",
            json!({
                "action": "click",
                "session_id": session_id,
                "selector": selector,
            }),
        ),
        Command::Type {
            session_id,
            selector,
            text,
        } => (
            "type",
            json!({
                "action": "type",
                "session_id": session_id,
                "selector": selector,
                "text": text,
            }),
        ),
        Command::GetUrl { session_id } => (
            "get_url",
            json!({
                "action": "get_url",
                "session_id": session_id,
            }),
        ),
        Command::GetCookies { session_id } => (
            "get_cookies",
            json!({
                "action": "get_cookies",
                "session_id": session_id,
            }),
        ),
    }
}

async fn run(command: Command, interactive: bool, hold_ms: u64) -> serde_json::Value {
    let tool = if interactive {
        ChromeTool::new_interactive()
    } else {
        ChromeTool::new()
    };

    let manager = ToolManager::new();
    manager.register(Arc::new(tool));

    let (action, request) = payload_for_command(&command);
    let result = manager.execute("chrome", request, make_ctx()).await;

    if hold_ms > 0 && matches!(command, Command::Open { .. }) && interactive {
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
