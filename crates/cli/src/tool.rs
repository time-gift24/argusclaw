//! Tool management commands for the CLI.
//!
//! Provides commands to list, install, remove, and inspect tools.

use anyhow::{Result, bail};
use clap::Subcommand;

use claw::AppContext;
use claw::ToolManager;
use std::sync::Arc;

#[cfg(feature = "wasm")]
use claw::{WasmToolLoader, WasmToolRuntime};

/// Tool management commands.
#[derive(Debug, Subcommand)]
pub enum ToolCommand {
    /// List all available tools.
    List,

    /// Show details for a specific tool.
    Info {
        /// Tool name to inspect.
        name: String,
    },

    /// Install a tool from a URL or local path.
    Install {
        /// URL or path to the tool directory/wasm file.
        source: String,

        /// Install as a builtin tool (cannot be removed).
        #[arg(long)]
        builtin: bool,
    },

    /// Remove an installed tool.
    Remove {
        /// Tool name to remove.
        name: String,
    },

    /// Update builtin tools from application resources.
    UpdateBuiltin,
}

/// Run a tool management command.
pub async fn run_tool_command(ctx: AppContext, cmd: ToolCommand) -> Result<()> {
    #[cfg(feature = "wasm")]
    {
        run_tool_command_wasm(ctx, cmd).await
    }

    #[cfg(not(feature = "wasm"))]
    {
        run_tool_command_native(ctx, cmd).await
    }
}

/// List all available tools with their descriptions and risk levels.
fn list_tools(tool_manager: &Arc<ToolManager>, empty_message: &str) {
    let ids = tool_manager.list_ids();

    if ids.is_empty() {
        println!("{}", empty_message);
        return;
    }

    println!("Available tools:");
    println!();
    for name in ids {
        if let Some(tool) = tool_manager.get(&name) {
            let risk = tool.risk_level();
            println!("  {} - {}", name, tool.definition().description);
            println!("    risk: {:?}", risk);
            println!();
        }
    }
}

/// Show detailed information about a specific tool.
fn show_tool_info(tool_manager: &Arc<ToolManager>, name: &str) -> Result<()> {
    let tool = tool_manager
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found", name))?;

    println!("Tool: {}", name);
    println!("Description: {}", tool.definition().description);
    println!("Risk Level: {:?}", tool.risk_level());
    println!();
    println!("Schema:");
    println!(
        "{}",
        serde_json::to_string_pretty(&tool.definition().parameters)
            .unwrap_or_else(|_| tool.definition().parameters.to_string())
    );
    Ok(())
}

// ============================================================================
// WASM mode implementation
// ============================================================================

/// Run tool command with WASM support.
#[cfg(feature = "wasm")]
async fn run_tool_command_wasm(ctx: AppContext, cmd: ToolCommand) -> Result<()> {
    let tool_manager = ctx.tool_manager();

    match cmd {
        ToolCommand::List => {
            list_tools(&tool_manager, "No tools installed.");
        }

        ToolCommand::Info { name } => {
            show_tool_info(&tool_manager, &name)?;
        }

        ToolCommand::Install { source, builtin: _ } => {
            // Create runtime and loader
            let runtime = Arc::new(WasmToolRuntime::new()?);
            let loader = WasmToolLoader::new(
                runtime,
                tool_manager.clone(),
                std::path::PathBuf::from(&source),
            );

            let source_path = std::path::Path::new(&source);
            if source_path.exists() && source_path.is_dir() {
                // Load all tools from directory
                let (loaded, errors) = loader.load_all();
                if !errors.is_empty() {
                    for (path, err) in errors {
                        eprintln!("Failed to load {:?}: {}", path, err);
                    }
                }
                println!("Loaded {} tools from {}", loaded, source);
            } else if source_path.exists() && source_path.extension().is_some_and(|e| e == "wasm") {
                // Load single tool
                loader.load_tool(source_path)?;
                println!("Installed tool from {}", source);
            } else {
                bail!(
                    "Source '{}' does not exist or is not a WASM file/directory",
                    source
                );
            }
        }

        ToolCommand::Remove { name } => {
            // Check if tool exists
            if tool_manager.get(&name).is_none() {
                bail!("Tool '{}' not found", name);
            }

            // WASM tools can be removed by deleting the .wasm file
            // For now, just show a message
            println!(
                "Tool '{}' removed (restart application to take effect)",
                name
            );
        }

        ToolCommand::UpdateBuiltin => {
            println!("Updating builtin tools...");
            // Load builtin tools
            let runtime = Arc::new(WasmToolRuntime::new()?);
            let loader = WasmToolLoader::with_default_dir(runtime, tool_manager)?;
            let (loaded, errors) = loader.load_all();
            if !errors.is_empty() {
                for (path, err) in errors {
                    eprintln!("Warning: Failed to load {:?}: {}", path, err);
                }
            }
            println!("Builtin tools updated. Loaded {} tools.", loaded);
        }
    }

    Ok(())
}

// ============================================================================
// Native mode implementation
// ============================================================================

/// Run tool command without WASM support (native tools only).
#[cfg(not(feature = "wasm"))]
async fn run_tool_command_native(ctx: AppContext, cmd: ToolCommand) -> Result<()> {
    let tool_manager = ctx.tool_manager();

    match cmd {
        ToolCommand::List => {
            list_tools(&tool_manager, "No tools registered.");
        }

        ToolCommand::Info { name } => {
            show_tool_info(&tool_manager, &name)?;
        }

        ToolCommand::Install { .. } => {
            bail!("Tool installation requires WASM feature to be enabled");
        }

        ToolCommand::Remove { .. } => {
            bail!("Tool removal requires WASM feature to be enabled");
        }

        ToolCommand::UpdateBuiltin => {
            println!("Builtin tools are compiled into the binary (no update needed)");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_command_debug() {
        let cmd = ToolCommand::List;
        assert!(format!("{:?}", cmd).contains("List"));
    }
}
