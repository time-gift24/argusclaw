//! ClapTool: converts a `clap::Command` into a `NamedTool`.
//!
//! Enables LLMs to progressively discover and invoke CLI subcommands:
//! 1. `{"action": "help"}` → list subcommands
//! 2. `{"action": "help", "subcommand": "install"}` → inspect parameters
//! 3. `{"action": "install", "args": {"package": "foo"}}` → execute

use std::sync::Arc;

use argus_protocol::{
    NamedTool, RiskLevel, ToolError, ToolExecutionContext,
    llm::ToolDefinition,
};
use async_trait::async_trait;
use clap::{Arg, ArgAction, ArgMatches, Command};

/// Execution backend implemented by consumers.
#[async_trait]
pub trait ClapExecutor: Send + Sync {
    /// Execute a parsed subcommand.
    async fn execute(
        &self,
        subcommand: &str,
        matches: &ArgMatches,
        tool_name: &str,
    ) -> Result<serde_json::Value, ToolError>;
}

/// Deserialized input from the LLM.
#[derive(Debug, serde::Deserialize)]
struct ClapToolInput {
    action: String,
    #[serde(default)]
    subcommand: Option<String>,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

/// Converts a `clap::Command` into a `NamedTool` for LLM consumption.
pub struct ClapTool {
    name: String,
    description: String,
    command: Command,
    executor: Arc<dyn ClapExecutor>,
    risk: RiskLevel,
}

impl std::fmt::Debug for ClapTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClapTool")
            .field("name", &self.name)
            .finish()
    }
}

impl ClapTool {
    /// Create a new ClapTool.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        command: Command,
        executor: Arc<dyn ClapExecutor>,
        risk: RiskLevel,
    ) -> Self {
        let command = command
            .disable_help_flag(true)
            .disable_version_flag(true)
            .infer_subcommands(true);
        Self {
            name: name.into(),
            description: description.into(),
            command,
            executor,
            risk,
        }
    }
}

#[async_trait]
impl NamedTool for ClapTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: build_definition_schema(&self.command),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        self.risk
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let parsed: ClapToolInput = serde_json::from_value(input).map_err(|e| {
            ToolError::ExecutionFailed {
                tool_name: self.name.clone(),
                reason: format!("invalid input: {e}"),
            }
        })?;

        match parsed.action.as_str() {
            "help" => execute_help(&self.command, parsed.subcommand),
            subcommand => execute_subcommand(
                &self.command,
                &self.executor,
                &self.name,
                subcommand,
                parsed.args,
            )
            .await,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers for clap API compatibility
// ---------------------------------------------------------------------------

/// Check if an arg is a flag (no value taken).
fn is_flag_arg(arg: &Arg) -> bool {
    matches!(
        arg.get_action(),
        ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count
    )
}

/// Check if an arg accepts multiple values.
fn is_multiple_arg(arg: &Arg) -> bool {
    matches!(arg.get_action(), ArgAction::Append) || arg.get_num_args().is_some_and(|r| r.max_values() > 1)
}

/// Get about description as String.
fn about_str(cmd: &Command) -> String {
    cmd.get_about().map(|s| s.to_string()).unwrap_or_default()
}

/// Get help text for an arg as String.
fn help_str(arg: &Arg) -> String {
    arg.get_help().map(|s| s.to_string()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Schema generation
// ---------------------------------------------------------------------------

/// Build the `oneOf` parameter schema for the tool definition.
fn build_definition_schema(command: &Command) -> serde_json::Value {
    let mut variants = Vec::new();

    // help variant
    variants.push(serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "const": "help",
                "description": "List subcommands or inspect a specific subcommand's parameters"
            },
            "subcommand": {
                "type": "string",
                "description": "Optional subcommand name to inspect parameters for"
            }
        },
        "required": ["action"],
        "additionalProperties": false
    }));

    // one variant per subcommand
    for sub in command.get_subcommands() {
        let sub_name = sub.get_name();
        let mut properties = serde_json::Map::new();
        let required = vec!["action"];

        properties.insert(
            "action".to_string(),
            serde_json::json!({
                "const": sub_name,
                "description": about_str(sub)
            }),
        );

        let args_schema = subcommand_args_schema(sub);
        if !args_schema.is_empty() {
            properties.insert("args".to_string(), serde_json::json!({
                "type": "object",
                "properties": args_schema,
                "description": format!("Arguments for the '{sub_name}' subcommand")
            }));
        }

        variants.push(serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }));
    }

    serde_json::json!({ "oneOf": variants })
}

/// Inspect a subcommand's arguments and produce a JSON Schema properties map.
fn subcommand_args_schema(sub: &Command) -> serde_json::Map<String, serde_json::Value> {
    let mut props = serde_json::Map::new();

    for arg in sub.get_arguments() {
        // skip args without long flag
        if arg.get_long().is_none() {
            continue;
        }

        let id = arg.get_id().as_str();
        let desc = help_str(arg);
        let schema = clap_arg_to_json_schema(arg, &desc);
        props.insert(id.to_string(), schema);
    }

    // Handle positional args
    for (idx, arg) in sub.get_positionals().enumerate() {
        let id = if arg.get_id().as_str() == clap::Id::default().as_str() {
            format!("arg{idx}")
        } else {
            arg.get_id().as_str().to_string()
        };
        let desc = help_str(arg);
        props.insert(id, clap_positional_to_json_schema(arg, &desc));
    }

    props
}

/// Convert a clap `Arg` (flag/option) to a JSON Schema value.
fn clap_arg_to_json_schema(arg: &Arg, desc: &str) -> serde_json::Value {
    if is_flag_arg(arg) {
        build_typed_schema(desc, "boolean")
    } else if is_multiple_arg(arg) {
        serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
            "description": desc
        })
    } else {
        build_typed_schema(desc, "string")
    }
}

/// Convert a positional arg to a JSON Schema value.
fn clap_positional_to_json_schema(arg: &Arg, desc: &str) -> serde_json::Value {
    if is_multiple_arg(arg) {
        serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
            "description": desc
        })
    } else {
        build_typed_schema(desc, "string")
    }
}

/// Build a simple typed schema with an optional description.
fn build_typed_schema(desc: &str, type_name: &str) -> serde_json::Value {
    let mut schema = serde_json::json!({ "type": type_name });
    if !desc.is_empty() {
        schema["description"] = serde_json::Value::String(desc.to_string());
    }
    schema
}

// ---------------------------------------------------------------------------
// Help execution
// ---------------------------------------------------------------------------

fn execute_help(
    command: &Command,
    subcommand: Option<String>,
) -> Result<serde_json::Value, ToolError> {
    match subcommand {
        None => {
            let subcommands: Vec<serde_json::Value> = command
                .get_subcommands()
                .filter(|s| !s.is_hide_set())
                .map(|s| {
                    let aliases: Vec<&str> = s.get_visible_aliases().collect();
                    serde_json::json!({
                        "name": s.get_name(),
                        "description": about_str(s),
                        "aliases": aliases
                    })
                })
                .collect();
            Ok(serde_json::json!({ "subcommands": subcommands }))
        }
        Some(name) => {
            let sub = command
                .get_subcommands()
                .find(|s| {
                    s.get_name() == name
                        || s.get_visible_aliases().any(|a| a == name)
                })
                .ok_or_else(|| ToolError::ExecutionFailed {
                    tool_name: "clap_tool".to_string(),
                    reason: format!("unknown subcommand: {name}"),
                })?;

            let params: Vec<serde_json::Value> = sub
                .get_arguments()
                .filter(|a| a.get_long().is_some() && !a.is_hide_set())
                .map(|a| {
                    let type_name = if is_flag_arg(a) {
                        "boolean"
                    } else {
                        "string"
                    };
                    serde_json::json!({
                        "name": a.get_id().as_str(),
                        "type": type_name,
                        "required": a.is_required_set(),
                        "description": help_str(a)
                    })
                })
                .collect();

            let aliases: Vec<&str> = sub.get_visible_aliases().collect();

            Ok(serde_json::json!({
                "subcommand": sub.get_name(),
                "description": about_str(sub),
                "aliases": aliases,
                "parameters": params
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// Subcommand execution
// ---------------------------------------------------------------------------

async fn execute_subcommand(
    command: &Command,
    executor: &Arc<dyn ClapExecutor>,
    tool_name: &str,
    subcommand: &str,
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, ToolError> {
    let sub = command
        .get_subcommands()
        .find(|s| {
            s.get_name() == subcommand
                || s.get_visible_aliases().any(|a| a == subcommand)
        })
        .ok_or_else(|| ToolError::ExecutionFailed {
            tool_name: tool_name.to_string(),
            reason: format!("unknown subcommand: {subcommand}"),
        })?;

    let argv = json_to_argv(subcommand, args.unwrap_or(serde_json::Value::Null), sub, tool_name)?;

    let cmd = command.clone();
    let matches = cmd.try_get_matches_from(argv).map_err(|e| {
        ToolError::ExecutionFailed {
            tool_name: tool_name.to_string(),
            reason: format!("argument parse error: {e}"),
        }
    })?;

    let sub_matches = matches
        .subcommand()
        .ok_or_else(|| ToolError::ExecutionFailed {
            tool_name: tool_name.to_string(),
            reason: "expected subcommand".to_string(),
        })?;

    executor
        .execute(sub_matches.0, sub_matches.1, tool_name)
        .await
}

/// Convert a JSON args object into CLI argv format.
fn json_to_argv(
    subcommand: &str,
    args: serde_json::Value,
    sub: &Command,
    tool_name: &str,
) -> Result<Vec<String>, ToolError> {
    let mut argv = vec![tool_name.to_string(), subcommand.to_string()];

    let obj = match args {
        serde_json::Value::Null | serde_json::Value::Object(_) => args,
        other => {
            return Err(ToolError::ExecutionFailed {
                tool_name: tool_name.to_string(),
                reason: format!("args must be an object, got: {other}"),
            })
        }
    };

    let map = match obj.as_object() {
        Some(m) => m,
        None => return Ok(argv),
    };

    // Build a lookup from arg ID to arg
    let known_args: Vec<&Arg> = sub
        .get_arguments()
        .filter(|a| a.get_long().is_some())
        .collect();

    for (key, value) in map {
        let arg = known_args
            .iter()
            .find(|a| a.get_id().as_str() == key)
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: tool_name.to_string(),
                reason: format!("unknown argument: {key}"),
            })?;

        let long = arg.get_long().expect("filtered for long above");

        if is_flag_arg(arg) {
            match value.as_bool() {
                Some(true) => argv.push(format!("--{long}")),
                Some(false) => {}
                None => {
                    return Err(ToolError::ExecutionFailed {
                        tool_name: tool_name.to_string(),
                        reason: format!("flag --{long} expects a boolean value"),
                    })
                }
            }
        } else if is_multiple_arg(arg) {
            let items = value
                .as_array()
                .ok_or_else(|| ToolError::ExecutionFailed {
                    tool_name: tool_name.to_string(),
                    reason: format!("--{long} expects an array"),
                })?;
            for item in items {
                argv.push(format!("--{long}"));
                argv.push(item.to_string().trim_matches('"').to_string());
            }
        } else {
            argv.push(format!("--{long}"));
            argv.push(value.to_string().trim_matches('"').to_string());
        }
    }

    // Handle positional args (keys matching positional arg IDs)
    for (idx, pos) in sub.get_positionals().enumerate() {
        let id = if pos.get_id().as_str() == clap::Id::default().as_str() {
            format!("arg{idx}")
        } else {
            pos.get_id().as_str().to_string()
        };
        if let Some(value) = map.get(&id) {
            if is_multiple_arg(pos) {
                if let Some(arr) = value.as_array() {
                    for item in arr {
                        argv.push(item.to_string().trim_matches('"').to_string());
                    }
                }
            } else {
                argv.push(value.to_string().trim_matches('"').to_string());
            }
        }
    }

    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_command() -> Command {
        Command::new("test-tool")
            .about("A test CLI tool")
            .subcommand(
                Command::new("install")
                    .about("Install a package")
                    .arg(
                        Arg::new("package")
                            .long("package")
                            .required(true)
                            .help("Package name to install"),
                    )
                    .arg(
                        Arg::new("force")
                            .long("force")
                            .action(ArgAction::SetTrue)
                            .help("Force installation"),
                    ),
            )
            .subcommand(
                Command::new("list")
                    .about("List packages")
                    .arg(
                        Arg::new("filter")
                            .long("filter")
                            .help("Filter pattern"),
                    ),
            )
            .subcommand(
                Command::new("remove")
                    .about("Remove a package")
                    .visible_alias("rm")
                    .arg(
                        Arg::new("package")
                            .long("package")
                            .required(true)
                            .help("Package name to remove"),
                    ),
            )
    }

    struct MockExecutor {
        response: serde_json::Value,
    }

    #[async_trait]
    impl ClapExecutor for MockExecutor {
        async fn execute(
            &self,
            subcommand: &str,
            _matches: &ArgMatches,
            _tool_name: &str,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({
                "subcommand": subcommand,
                "result": self.response
            }))
        }
    }

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (pipe_tx, _) = tokio::sync::broadcast::channel(8);
        let (control_tx, _) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(ToolExecutionContext {
            thread_id: argus_protocol::ids::ThreadId::new(),
            pipe_tx,
            control_tx,
        })
    }

    #[test]
    fn definition_uses_one_of_schema() {
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            sample_command(),
            Arc::new(MockExecutor {
                response: serde_json::json!("ok"),
            }),
            RiskLevel::Medium,
        );

        let def = tool.definition();
        assert_eq!(def.name, "test-cli");
        let variants = def.parameters["oneOf"]
            .as_array()
            .expect("should have oneOf");
        // help + install + list + remove = 4
        assert_eq!(variants.len(), 4);

        // help variant
        let help = variants
            .iter()
            .find(|v| v["properties"]["action"]["const"] == "help")
            .expect("help variant");
        assert!(help["properties"]["subcommand"].is_object());
    }

    #[tokio::test]
    async fn help_action_lists_subcommands() {
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            sample_command(),
            Arc::new(MockExecutor {
                response: serde_json::json!("ok"),
            }),
            RiskLevel::Low,
        );

        let result = tool
            .execute(serde_json::json!({"action": "help"}), make_ctx())
            .await
            .expect("help should succeed");

        let subs = result["subcommands"].as_array().expect("subcommands array");
        assert_eq!(subs.len(), 3);

        let install = subs.iter().find(|s| s["name"] == "install").expect("install");
        assert_eq!(install["description"], "Install a package");

        let remove = subs.iter().find(|s| s["name"] == "remove").expect("remove");
        assert_eq!(remove["aliases"], serde_json::json!(["rm"]));
    }

    #[tokio::test]
    async fn help_with_subcommand_shows_parameters() {
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            sample_command(),
            Arc::new(MockExecutor {
                response: serde_json::json!("ok"),
            }),
            RiskLevel::Low,
        );

        let result = tool
            .execute(
                serde_json::json!({"action": "help", "subcommand": "install"}),
                make_ctx(),
            )
            .await
            .expect("help install should succeed");

        assert_eq!(result["subcommand"], "install");
        let params = result["parameters"].as_array().expect("parameters array");
        assert_eq!(params.len(), 2); // package + force

        let pkg = params
            .iter()
            .find(|p| p["name"] == "package")
            .expect("package param");
        assert_eq!(pkg["type"], "string");
        assert_eq!(pkg["required"], true);
    }

    #[tokio::test]
    async fn execute_subcommand_with_args() {
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            sample_command(),
            Arc::new(MockExecutor {
                response: serde_json::json!("installed"),
            }),
            RiskLevel::Medium,
        );

        let result = tool
            .execute(
                serde_json::json!({
                    "action": "install",
                    "args": {
                        "package": "foo",
                        "force": true
                    }
                }),
                make_ctx(),
            )
            .await
            .expect("install should succeed");

        assert_eq!(result["subcommand"], "install");
        assert_eq!(result["result"], "installed");
    }

    #[tokio::test]
    async fn execute_with_alias_name() {
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            sample_command(),
            Arc::new(MockExecutor {
                response: serde_json::json!("removed"),
            }),
            RiskLevel::Medium,
        );

        let result = tool
            .execute(
                serde_json::json!({
                    "action": "rm",
                    "args": { "package": "bar" }
                }),
                make_ctx(),
            )
            .await
            .expect("rm alias should succeed");

        assert_eq!(result["subcommand"], "remove");
    }

    #[tokio::test]
    async fn unknown_subcommand_returns_error() {
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            sample_command(),
            Arc::new(MockExecutor {
                response: serde_json::json!("ok"),
            }),
            RiskLevel::Low,
        );

        let result = tool
            .execute(
                serde_json::json!({"action": "nonexistent"}),
                make_ctx(),
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            ToolError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("unknown subcommand"));
            }
            other => panic!("expected ExecutionFailed, got: {other}"),
        }
    }

    #[tokio::test]
    async fn help_for_unknown_subcommand_returns_error() {
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            sample_command(),
            Arc::new(MockExecutor {
                response: serde_json::json!("ok"),
            }),
            RiskLevel::Low,
        );

        let result = tool
            .execute(
                serde_json::json!({"action": "help", "subcommand": "nope"}),
                make_ctx(),
            )
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn name_and_risk_level() {
        let tool = ClapTool::new(
            "my-cli",
            "desc",
            sample_command(),
            Arc::new(MockExecutor {
                response: serde_json::json!("ok"),
            }),
            RiskLevel::High,
        );
        assert_eq!(tool.name(), "my-cli");
        assert_eq!(tool.risk_level(), RiskLevel::High);
    }
}
