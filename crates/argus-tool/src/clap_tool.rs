//! ClapTool: converts a `clap::Command` into a `NamedTool`.
//!
//! Enables LLMs to progressively discover and invoke CLI subcommands:
//! 1. `{"action": "help"}` → list subcommands
//! 2. `{"action": "help", "subcommand": "install"}` → inspect parameters
//! 3. `{"action": "install", "args": {"package": "foo"}}` → execute

use std::{collections::HashSet, sync::Arc};

use argus_protocol::{NamedTool, RiskLevel, ToolError, ToolExecutionContext, llm::ToolDefinition};
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
        ctx: Arc<ToolExecutionContext>,
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
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let parsed: ClapToolInput =
            serde_json::from_value(input).map_err(|e| ToolError::ExecutionFailed {
                tool_name: self.name.clone(),
                reason: format!("invalid input: {e}"),
            })?;

        match parsed.action.as_str() {
            "help" => execute_help(&self.command, &self.name, parsed.subcommand),
            subcommand => {
                execute_subcommand(
                    &self.command,
                    &self.executor,
                    &self.name,
                    ctx,
                    subcommand,
                    parsed.args,
                )
                .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers for clap API compatibility
// ---------------------------------------------------------------------------

/// Check if an arg accepts multiple values.
fn is_multiple_arg(arg: &Arg) -> bool {
    matches!(arg.get_action(), ArgAction::Append)
        || arg.get_num_args().is_some_and(|r| r.max_values() > 1)
}

/// Get about description as String.
fn about_str(cmd: &Command) -> String {
    cmd.get_about().map(|s| s.to_string()).unwrap_or_default()
}

/// Get help text for an arg as String.
fn help_str(arg: &Arg) -> String {
    arg.get_help().map(|s| s.to_string()).unwrap_or_default()
}

/// Whether a subcommand should be discoverable and executable via the tool API.
fn is_visible_subcommand(sub: &Command) -> bool {
    !sub.is_hide_set()
}

/// Whether an argument should be exposed in schemas/help and accepted from JSON input.
fn is_visible_arg(arg: &Arg) -> bool {
    !arg.is_hide_set()
}

/// Stable JSON key for a positional argument.
fn positional_arg_id(idx: usize, arg: &Arg) -> String {
    if arg.get_id().as_str() == clap::Id::default().as_str() {
        format!("arg{idx}")
    } else {
        arg.get_id().as_str().to_string()
    }
}

/// Resolve a visible subcommand by canonical name or visible alias.
fn find_visible_subcommand<'a>(command: &'a Command, name: &str) -> Option<&'a Command> {
    command.get_subcommands().find(|sub| {
        is_visible_subcommand(sub)
            && (sub.get_name() == name || sub.get_visible_aliases().any(|alias| alias == name))
    })
}

fn visible_action_names(sub: &Command) -> Vec<String> {
    let mut names = vec![sub.get_name().to_string()];
    names.extend(sub.get_visible_aliases().map(ToString::to_string));
    names
}

fn action_schema(sub: &Command) -> serde_json::Value {
    let mut schema = serde_json::json!({
        "description": about_str(sub)
    });
    let names = visible_action_names(sub);
    if names.len() == 1 {
        schema["const"] = serde_json::Value::String(names[0].clone());
    } else {
        schema["enum"] = serde_json::Value::Array(names.into_iter().map(Into::into).collect());
    }
    schema
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArgSchemaKind {
    BooleanFlag,
    CountFlag,
    RepeatedValues,
    ScalarValue,
}

fn arg_schema_kind(arg: &Arg) -> ArgSchemaKind {
    match arg.get_action() {
        ArgAction::SetTrue | ArgAction::SetFalse => ArgSchemaKind::BooleanFlag,
        ArgAction::Count => ArgSchemaKind::CountFlag,
        _ if is_multiple_arg(arg) => ArgSchemaKind::RepeatedValues,
        _ => ArgSchemaKind::ScalarValue,
    }
}

#[derive(Default)]
struct ArgsSchema {
    properties: serde_json::Map<String, serde_json::Value>,
    required: Vec<String>,
}

// ---------------------------------------------------------------------------
// Schema generation
// ---------------------------------------------------------------------------

/// Build the `oneOf` parameter schema for the tool definition.
fn build_definition_schema(command: &Command) -> serde_json::Value {
    let mut variants = Vec::new();
    let visible_subcommands: Vec<&Command> = command
        .get_subcommands()
        .filter(|sub| is_visible_subcommand(sub))
        .collect();

    // help variant
    let mut subcommand_schema = serde_json::json!({
        "type": "string",
        "description": "Optional subcommand name to inspect parameters for"
    });
    let subcommand_names: Vec<String> = visible_subcommands
        .iter()
        .flat_map(|sub| visible_action_names(sub))
        .collect();
    if !subcommand_names.is_empty() {
        subcommand_schema["enum"] =
            serde_json::Value::Array(subcommand_names.into_iter().map(Into::into).collect());
    }
    variants.push(serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "const": "help",
                "description": "List subcommands or inspect a specific subcommand's parameters"
            },
            "subcommand": subcommand_schema
        },
        "required": ["action"],
        "additionalProperties": false
    }));

    // one variant per subcommand
    for sub in visible_subcommands {
        let sub_name = sub.get_name();
        let mut properties = serde_json::Map::new();
        let mut required = vec!["action"];

        properties.insert("action".to_string(), action_schema(sub));

        let args_schema = subcommand_args_schema(sub);
        if !args_schema.properties.is_empty() {
            let mut args_value = serde_json::json!({
                "type": "object",
                "properties": args_schema.properties,
                "description": format!("Arguments for the '{sub_name}' subcommand"),
                "additionalProperties": false
            });
            if !args_schema.required.is_empty() {
                args_value["required"] = serde_json::Value::Array(
                    args_schema
                        .required
                        .iter()
                        .cloned()
                        .map(Into::into)
                        .collect(),
                );
                required.push("args");
            }
            properties.insert("args".to_string(), args_value);
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

/// Inspect a subcommand's arguments and produce a JSON Schema object definition.
fn subcommand_args_schema(sub: &Command) -> ArgsSchema {
    let mut schema = ArgsSchema::default();

    for arg in sub.get_arguments() {
        // skip args without long flag
        if arg.get_long().is_none() || !is_visible_arg(arg) {
            continue;
        }

        let id = arg.get_id().as_str();
        let desc = help_str(arg);
        schema
            .properties
            .insert(id.to_string(), clap_arg_to_json_schema(arg, &desc));
        if arg.is_required_set() {
            schema.required.push(id.to_string());
        }
    }

    // Handle positional args
    for (idx, arg) in sub
        .get_positionals()
        .filter(|arg| is_visible_arg(arg))
        .enumerate()
    {
        let id = positional_arg_id(idx, arg);
        let desc = help_str(arg);
        schema
            .properties
            .insert(id.clone(), clap_positional_to_json_schema(arg, &desc));
        if arg.is_required_set() {
            schema.required.push(id);
        }
    }

    schema
}

/// Convert a clap `Arg` (flag/option) to a JSON Schema value.
fn clap_arg_to_json_schema(arg: &Arg, desc: &str) -> serde_json::Value {
    match arg_schema_kind(arg) {
        ArgSchemaKind::BooleanFlag => build_typed_schema(desc, "boolean"),
        ArgSchemaKind::CountFlag => {
            let mut schema = build_typed_schema(desc, "integer");
            schema["minimum"] = serde_json::json!(0);
            schema
        }
        ArgSchemaKind::RepeatedValues => serde_json::json!({
            "type": "array",
            "items": { "type": "string" },
            "description": desc
        }),
        ArgSchemaKind::ScalarValue => build_typed_schema(desc, "string"),
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
    tool_name: &str,
    subcommand: Option<String>,
) -> Result<serde_json::Value, ToolError> {
    match subcommand {
        None => {
            let subcommands: Vec<serde_json::Value> = command
                .get_subcommands()
                .filter(|sub| is_visible_subcommand(sub))
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
            let sub = find_visible_subcommand(command, &name).ok_or_else(|| {
                ToolError::ExecutionFailed {
                    tool_name: tool_name.to_string(),
                    reason: format!("unknown subcommand: {name}"),
                }
            })?;

            let mut params: Vec<serde_json::Value> = sub
                .get_arguments()
                .filter(|arg| arg.get_long().is_some() && is_visible_arg(arg))
                .map(|a| {
                    let type_name = clap_arg_to_json_schema(a, &help_str(a))["type"]
                        .as_str()
                        .unwrap_or("string")
                        .to_string();
                    serde_json::json!({
                        "name": a.get_id().as_str(),
                        "type": type_name,
                        "required": a.is_required_set(),
                        "kind": "option",
                        "description": help_str(a)
                    })
                })
                .collect();
            params.extend(
                sub.get_positionals()
                    .filter(|arg| is_visible_arg(arg))
                    .enumerate()
                    .map(|(idx, arg)| {
                        let type_name = clap_positional_to_json_schema(arg, &help_str(arg))["type"]
                            .as_str()
                            .unwrap_or("string")
                            .to_string();
                        serde_json::json!({
                            "name": positional_arg_id(idx, arg),
                            "type": type_name,
                            "required": arg.is_required_set(),
                            "kind": "positional",
                            "description": help_str(arg)
                        })
                    }),
            );

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
    ctx: Arc<ToolExecutionContext>,
    subcommand: &str,
    args: Option<serde_json::Value>,
) -> Result<serde_json::Value, ToolError> {
    let sub =
        find_visible_subcommand(command, subcommand).ok_or_else(|| ToolError::ExecutionFailed {
            tool_name: tool_name.to_string(),
            reason: format!("unknown subcommand: {subcommand}"),
        })?;

    let argv = json_to_argv(
        subcommand,
        args.unwrap_or(serde_json::Value::Null),
        sub,
        tool_name,
    )?;

    let cmd = command.clone();
    let matches = cmd
        .try_get_matches_from(argv)
        .map_err(|e| ToolError::ExecutionFailed {
            tool_name: tool_name.to_string(),
            reason: format!("argument parse error: {e}"),
        })?;

    let sub_matches = matches
        .subcommand()
        .ok_or_else(|| ToolError::ExecutionFailed {
            tool_name: tool_name.to_string(),
            reason: "expected subcommand".to_string(),
        })?;

    executor
        .execute(sub_matches.0, sub_matches.1, tool_name, ctx)
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
            });
        }
    };

    let map = match obj.as_object() {
        Some(m) => m,
        None => return Ok(argv),
    };

    let known_options: Vec<&Arg> = sub
        .get_arguments()
        .filter(|arg| arg.get_long().is_some() && is_visible_arg(arg))
        .collect();
    let known_positionals: Vec<(String, &Arg)> = sub
        .get_positionals()
        .filter(|arg| is_visible_arg(arg))
        .enumerate()
        .map(|(idx, arg)| (positional_arg_id(idx, arg), arg))
        .collect();
    let mut known_keys: HashSet<String> = known_options
        .iter()
        .map(|arg| arg.get_id().as_str().to_string())
        .collect();
    known_keys.extend(known_positionals.iter().map(|(id, _)| id.clone()));

    for arg in known_options {
        let key = arg.get_id().as_str();
        let Some(value) = map.get(key) else {
            continue;
        };
        let long = arg.get_long().expect("filtered for long above");

        match arg_schema_kind(arg) {
            ArgSchemaKind::BooleanFlag => {
                let bool_value = value.as_bool().ok_or_else(|| ToolError::ExecutionFailed {
                    tool_name: tool_name.to_string(),
                    reason: format!("flag --{long} expects a boolean value"),
                })?;
                let should_emit = match arg.get_action() {
                    ArgAction::SetFalse => !bool_value,
                    _ => bool_value,
                };
                if should_emit {
                    argv.push(format!("--{long}"));
                }
            }
            ArgSchemaKind::CountFlag => {
                let count = value.as_u64().ok_or_else(|| ToolError::ExecutionFailed {
                    tool_name: tool_name.to_string(),
                    reason: format!("flag --{long} expects a non-negative integer"),
                })?;
                for _ in 0..count {
                    argv.push(format!("--{long}"));
                }
            }
            ArgSchemaKind::RepeatedValues => {
                let items = value.as_array().ok_or_else(|| ToolError::ExecutionFailed {
                    tool_name: tool_name.to_string(),
                    reason: format!("--{long} expects an array"),
                })?;
                for item in items {
                    argv.push(format!(
                        "--{long}={}",
                        json_scalar_to_argv_value(item, &format!("--{long}"), tool_name)?
                    ));
                }
            }
            ArgSchemaKind::ScalarValue => {
                argv.push(format!(
                    "--{long}={}",
                    json_scalar_to_argv_value(value, &format!("--{long}"), tool_name)?
                ));
            }
        }
    }

    let mut positional_values = Vec::new();
    for (id, pos) in known_positionals {
        let Some(value) = map.get(&id) else {
            continue;
        };
        if is_multiple_arg(pos) {
            let arr = value.as_array().ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: tool_name.to_string(),
                reason: format!("positional argument {id} expects an array"),
            })?;
            for item in arr {
                positional_values.push(json_scalar_to_argv_value(item, &id, tool_name)?);
            }
        } else {
            positional_values.push(json_scalar_to_argv_value(value, &id, tool_name)?);
        }
    }

    for key in map.keys() {
        if !known_keys.contains(key) {
            return Err(ToolError::ExecutionFailed {
                tool_name: tool_name.to_string(),
                reason: format!("unknown argument: {key}"),
            });
        }
    }

    if !positional_values.is_empty() {
        argv.push("--".to_string());
        argv.extend(positional_values);
    }

    Ok(argv)
}

fn json_scalar_to_argv_value(
    value: &serde_json::Value,
    arg_name: &str,
    tool_name: &str,
) -> Result<String, ToolError> {
    match value {
        serde_json::Value::String(string) => Ok(string.clone()),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        serde_json::Value::Bool(boolean) => Ok(boolean.to_string()),
        serde_json::Value::Null => Err(ToolError::ExecutionFailed {
            tool_name: tool_name.to_string(),
            reason: format!("{arg_name} does not accept null"),
        }),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Err(ToolError::ExecutionFailed {
                tool_name: tool_name.to_string(),
                reason: format!("{arg_name} expects a scalar value"),
            })
        }
    }
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
                    .arg(Arg::new("filter").long("filter").help("Filter pattern")),
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

    fn variant_allows_action(variant: &serde_json::Value, action: &str) -> bool {
        match &variant["properties"]["action"] {
            serde_json::Value::Object(map) => {
                map.get("const")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| value == action)
                    || map
                        .get("enum")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|values| values.iter().any(|value| value == action))
            }
            _ => false,
        }
    }

    fn command_with_positionals_and_hidden() -> Command {
        Command::new("test-tool")
            .subcommand(
                Command::new("show")
                    .about("Show a package")
                    .arg(Arg::new("target").required(true).help("Target package"))
                    .arg(
                        Arg::new("secret")
                            .long("secret")
                            .hide(true)
                            .help("Hidden internal flag"),
                    ),
            )
            .subcommand(
                Command::new("internal")
                    .about("Hidden internal subcommand")
                    .hide(true)
                    .arg(
                        Arg::new("token")
                            .long("token")
                            .required(true)
                            .help("Internal token"),
                    ),
            )
    }

    fn command_with_count_flag() -> Command {
        Command::new("test-tool").subcommand(
            Command::new("search").about("Search packages").arg(
                Arg::new("verbose")
                    .long("verbose")
                    .action(ArgAction::Count)
                    .help("Increase verbosity"),
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
            _ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({
                "subcommand": subcommand,
                "result": self.response
            }))
        }
    }

    struct ContextCapturingExecutor {
        seen_thread_id: std::sync::Mutex<Option<argus_protocol::ids::ThreadId>>,
    }

    #[async_trait]
    impl ClapExecutor for ContextCapturingExecutor {
        async fn execute(
            &self,
            _subcommand: &str,
            _matches: &ArgMatches,
            _tool_name: &str,
            ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
            *self.seen_thread_id.lock().expect("lock poisoned") = Some(ctx.thread_id);
            Ok(serde_json::json!({"ok": true}))
        }
    }

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (pipe_tx, _) = tokio::sync::broadcast::channel(8);
        Arc::new(ToolExecutionContext {
            thread_id: argus_protocol::ids::ThreadId::new(),
            agent_id: None,
            pipe_tx,
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
            .find(|variant| variant_allows_action(variant, "help"))
            .expect("help variant");
        assert!(help["properties"]["subcommand"].is_object());
    }

    #[test]
    fn definition_includes_visible_aliases_in_action_schema() {
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
        let variants = def.parameters["oneOf"]
            .as_array()
            .expect("should have oneOf");
        let remove = variants
            .iter()
            .find(|variant| variant_allows_action(variant, "remove"))
            .expect("remove variant");

        assert_eq!(
            remove["properties"]["action"]["enum"],
            serde_json::json!(["remove", "rm"])
        );
        assert!(
            def.parameters["oneOf"]
                .as_array()
                .expect("should have oneOf")
                .iter()
                .any(|variant| variant_allows_action(variant, "rm"))
        );
    }

    #[test]
    fn definition_marks_required_args_and_restricts_unknown_fields() {
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
        let variants = def.parameters["oneOf"]
            .as_array()
            .expect("should have oneOf");
        let install = variants
            .iter()
            .find(|variant| variant_allows_action(variant, "install"))
            .expect("install variant");

        assert_eq!(
            install["properties"]["args"]["required"],
            serde_json::json!(["package"])
        );
        assert_eq!(
            install["properties"]["args"]["additionalProperties"],
            serde_json::json!(false)
        );
        assert_eq!(install["required"], serde_json::json!(["action", "args"]));
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

        let install = subs
            .iter()
            .find(|s| s["name"] == "install")
            .expect("install");
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

    #[test]
    fn json_to_argv_accepts_positional_arguments() {
        let command = command_with_positionals_and_hidden();
        let sub = command
            .get_subcommands()
            .find(|sub| sub.get_name() == "show")
            .expect("show subcommand");

        let argv = json_to_argv(
            "show",
            serde_json::json!({
                "target": "foo"
            }),
            sub,
            "test-cli",
        )
        .expect("positionals should be supported");

        assert_eq!(argv, vec!["test-cli", "show", "--", "foo"]);
    }

    #[test]
    fn json_to_argv_binds_option_values_without_reparsing_them_as_flags() {
        let command = sample_command();
        let sub = command
            .get_subcommands()
            .find(|sub| sub.get_name() == "install")
            .expect("install subcommand");

        let argv = json_to_argv(
            "install",
            serde_json::json!({
                "package": "--not-a-flag"
            }),
            sub,
            "test-cli",
        )
        .expect("option values should be encoded safely");

        assert_eq!(argv, vec!["test-cli", "install", "--package=--not-a-flag"]);
    }

    #[test]
    fn definition_omits_hidden_subcommands_and_args() {
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            command_with_positionals_and_hidden(),
            Arc::new(MockExecutor {
                response: serde_json::json!("ok"),
            }),
            RiskLevel::Medium,
        );

        let def = tool.definition();
        let variants = def.parameters["oneOf"]
            .as_array()
            .expect("should have oneOf");

        assert!(
            variants
                .iter()
                .all(|variant| !variant_allows_action(variant, "internal"))
        );

        let show = variants
            .iter()
            .find(|variant| variant_allows_action(variant, "show"))
            .expect("show variant");
        assert!(show["properties"]["args"]["properties"]["secret"].is_null());
        assert!(show["properties"]["args"]["properties"]["target"].is_object());
    }

    #[test]
    fn count_flags_use_integer_schema_and_repeat_in_argv() {
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            command_with_count_flag(),
            Arc::new(MockExecutor {
                response: serde_json::json!("ok"),
            }),
            RiskLevel::Medium,
        );

        let def = tool.definition();
        let variants = def.parameters["oneOf"]
            .as_array()
            .expect("should have oneOf");
        let search = variants
            .iter()
            .find(|variant| variant_allows_action(variant, "search"))
            .expect("search variant");
        assert_eq!(
            search["properties"]["args"]["properties"]["verbose"]["type"],
            "integer"
        );

        let command = command_with_count_flag();
        let sub = command
            .get_subcommands()
            .find(|sub| sub.get_name() == "search")
            .expect("search subcommand");
        let argv = json_to_argv(
            "search",
            serde_json::json!({
                "verbose": 2
            }),
            sub,
            "test-cli",
        )
        .expect("count flags should be supported");

        assert_eq!(argv, vec!["test-cli", "search", "--verbose", "--verbose"]);
    }

    #[tokio::test]
    async fn hidden_subcommands_are_not_executable() {
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            command_with_positionals_and_hidden(),
            Arc::new(MockExecutor {
                response: serde_json::json!("ok"),
            }),
            RiskLevel::Medium,
        );

        let result = tool
            .execute(
                serde_json::json!({
                    "action": "internal",
                    "args": {
                        "token": "secret"
                    }
                }),
                make_ctx(),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn executor_receives_tool_execution_context() {
        let executor = Arc::new(ContextCapturingExecutor {
            seen_thread_id: std::sync::Mutex::new(None),
        });
        let tool = ClapTool::new(
            "test-cli",
            "Test CLI",
            sample_command(),
            executor.clone(),
            RiskLevel::Medium,
        );
        let ctx = make_ctx();
        let expected_thread_id = ctx.thread_id;

        tool.execute(
            serde_json::json!({
                "action": "install",
                "args": {
                    "package": "foo"
                }
            }),
            ctx,
        )
        .await
        .expect("execution should succeed");

        let seen = *executor.seen_thread_id.lock().expect("lock poisoned");
        assert_eq!(seen, Some(expected_thread_id));
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
            .execute(serde_json::json!({"action": "nonexistent"}), make_ctx())
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
