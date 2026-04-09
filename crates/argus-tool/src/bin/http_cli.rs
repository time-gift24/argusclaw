use std::sync::Arc;

use argus_protocol::ids::ThreadId;
use argus_protocol::tool::ToolExecutionContext;
use argus_tool::{HttpTool, ToolManager};
use clap::{Parser, Subcommand};
use serde_json::json;
use tokio::sync::broadcast;

#[derive(Debug, Parser)]
#[command(
    name = "argus-http-cli",
    version,
    about = "Smoke test for HTTP tool network capabilities"
)]
struct Cli {
    /// Pretty-print JSON output
    #[arg(long, global = true, default_value_t = false)]
    pretty: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// GET request
    Get {
        #[arg(long)]
        url: String,
        /// Custom header (key:value), repeatable
        #[arg(long = "header", value_parser = parse_header)]
        headers: Vec<(String, String)>,
        /// Timeout in seconds (default 30, max 300)
        #[arg(long)]
        timeout: Option<u64>,
        /// Save response body to file
        #[arg(long)]
        save_to: Option<String>,
    },
    /// POST request
    Post {
        #[arg(long)]
        url: String,
        /// Request body (raw string)
        #[arg(long)]
        body: Option<String>,
        #[arg(long = "header", value_parser = parse_header)]
        headers: Vec<(String, String)>,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        save_to: Option<String>,
    },
    /// PUT request
    Put {
        #[arg(long)]
        url: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long = "header", value_parser = parse_header)]
        headers: Vec<(String, String)>,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        save_to: Option<String>,
    },
    /// DELETE request
    Delete {
        #[arg(long)]
        url: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long = "header", value_parser = parse_header)]
        headers: Vec<(String, String)>,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        save_to: Option<String>,
    },
    /// HEAD request
    Head {
        #[arg(long)]
        url: String,
        #[arg(long = "header", value_parser = parse_header)]
        headers: Vec<(String, String)>,
        #[arg(long)]
        timeout: Option<u64>,
    },
}

fn parse_header(s: &str) -> Result<(String, String), String> {
    let (key, value) = s
        .split_once(':')
        .ok_or_else(|| format!("header must be key:value, got: {s}"))?;
    Ok((key.trim().to_string(), value.trim().to_string()))
}

fn make_ctx() -> Arc<ToolExecutionContext> {
    let (pipe_tx, _) = broadcast::channel(16);
    Arc::new(ToolExecutionContext {
        thread_id: ThreadId::new(),
        agent_id: None,
        pipe_tx,
    })
}

fn build_request(cmd: &Command) -> serde_json::Value {
    let (method, url, headers, body, timeout, save_to) = match cmd {
        Command::Get {
            url,
            headers,
            timeout,
            save_to,
        } => ("GET", url, headers, &None, timeout, save_to),
        Command::Post {
            url,
            body,
            headers,
            timeout,
            save_to,
        } => ("POST", url, headers, body, timeout, save_to),
        Command::Put {
            url,
            body,
            headers,
            timeout,
            save_to,
        } => ("PUT", url, headers, body, timeout, save_to),
        Command::Delete {
            url,
            body,
            headers,
            timeout,
            save_to,
        } => ("DELETE", url, headers, body, timeout, save_to),
        Command::Head {
            url,
            headers,
            timeout,
        } => ("HEAD", url, headers, &None, timeout, &None),
    };

    let headers_map: serde_json::Map<String, serde_json::Value> = headers
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();

    let mut req = json!({
        "url": url,
        "method": method,
        "headers": headers_map,
    });

    if let Some(b) = body {
        req["body"] = json!(b);
    }
    if let Some(t) = timeout {
        req["timeout"] = json!(t);
    }
    if let Some(s) = save_to {
        req["saveTo"] = json!(s);
    }

    req
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let manager = ToolManager::new();
    manager.register(Arc::new(HttpTool::new()));

    let request = build_request(&cli.command);
    let result = manager.execute("http", request, make_ctx()).await;

    let output = match result {
        Ok(result) => json!({ "ok": true, "result": result }),
        Err(error) => json!({ "ok": false, "error": error.to_string() }),
    };

    let formatted = if cli.pretty {
        serde_json::to_string_pretty(&output).unwrap()
    } else {
        serde_json::to_string(&output).unwrap()
    };

    println!("{formatted}");
}
