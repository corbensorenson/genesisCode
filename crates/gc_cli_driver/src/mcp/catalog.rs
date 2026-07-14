use std::any::TypeId;
use std::collections::BTreeSet;

use clap::{Arg, ArgAction, Command, CommandFactory};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::super::*;

pub(super) const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
pub(super) const MCP_INTERFACE_SCHEMA: &str = "genesis/mcp-interface-v0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueKind {
    Bool,
    Integer,
    Number,
    String,
    StringArray,
}

#[derive(Debug, Clone)]
pub(super) struct ArgBinding {
    pub(super) id: String,
    pub(super) long: Option<String>,
    pub(super) positional: bool,
    pub(super) position: Option<usize>,
    pub(super) depth: usize,
    pub(super) required: bool,
    pub(super) kind: ValueKind,
    pub(super) bool_flag_value: bool,
    pub(super) schema: Value,
}

#[derive(Debug, Clone)]
pub(super) struct ToolBinding {
    pub(super) name: &'static str,
    pub(super) route: Vec<&'static str>,
    pub(super) arguments: Vec<ArgBinding>,
    pub(super) definition: Value,
}

#[derive(Clone, Copy)]
struct RoutePolicy {
    name: &'static str,
    route: &'static [&'static str],
    allowed: &'static [&'static str],
    forced_required: &'static [&'static str],
    annotations: ToolAnnotations,
}

#[derive(Clone, Copy)]
struct ToolAnnotations {
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    open_world: bool,
}

const READ_ONLY: ToolAnnotations = ToolAnnotations {
    read_only: true,
    destructive: false,
    idempotent: true,
    open_world: false,
};
const IDEMPOTENT_WRITE: ToolAnnotations = ToolAnnotations {
    read_only: false,
    destructive: true,
    idempotent: true,
    open_world: false,
};
const OPEN_WORLD_WRITE: ToolAnnotations = ToolAnnotations {
    read_only: false,
    destructive: true,
    idempotent: false,
    open_world: true,
};
const NON_IDEMPOTENT_WRITE: ToolAnnotations = ToolAnnotations {
    read_only: false,
    destructive: true,
    idempotent: false,
    open_world: false,
};

const ROUTES: &[RoutePolicy] = &[
    route("parse", &["parse"], &["file", "engine"], &[], READ_ONLY),
    route(
        "format",
        &["fmt"],
        &["file", "check", "engine"],
        &[],
        IDEMPOTENT_WRITE,
    ),
    route(
        "check",
        &["typecheck"],
        &["pkg", "strict_sound"],
        &[],
        READ_ONLY,
    ),
    route(
        "run",
        &["run"],
        &["file", "engine", "caps", "log"],
        &[],
        OPEN_WORLD_WRITE,
    ),
    route("test", &["test"], &["pkg", "caps"], &[], OPEN_WORLD_WRITE),
    route(
        "explain",
        &["explain"],
        &["file", "engine", "contract", "msg"],
        &[],
        READ_ONLY,
    ),
    route(
        "search-symbol",
        &["agent-index"],
        &["search_symbol", "max_results"],
        &["search_symbol"],
        READ_ONLY,
    ),
    route(
        "get-card",
        &["agent-index"],
        &["card"],
        &["card"],
        READ_ONLY,
    ),
    route(
        "diff",
        &["vcs", "diff"],
        &["caps", "log", "base", "to", "out", "no_store"],
        &[],
        IDEMPOTENT_WRITE,
    ),
    route(
        "apply-patch",
        &["apply-patch"],
        &["patch", "pkg", "caps"],
        &[],
        NON_IDEMPOTENT_WRITE,
    ),
    route(
        "verify",
        &["verify"],
        &["pkg", "acceptance", "policy", "signatures", "scan_store"],
        &[],
        READ_ONLY,
    ),
    route(
        "replay",
        &["replay"],
        &["file", "engine", "log", "store"],
        &[],
        READ_ONLY,
    ),
    route(
        "session-abort",
        &["session", "abort"],
        &["pkg", "session"],
        &[],
        IDEMPOTENT_WRITE,
    ),
    route(
        "session-apply",
        &["session", "apply"],
        &["pkg", "session"],
        &[],
        NON_IDEMPOTENT_WRITE,
    ),
    route(
        "session-begin",
        &["session", "begin"],
        &["pkg", "session"],
        &[],
        NON_IDEMPOTENT_WRITE,
    ),
    route(
        "session-stage",
        &["session", "stage"],
        &["pkg", "session", "patch", "caps"],
        &[],
        NON_IDEMPOTENT_WRITE,
    ),
    route(
        "session-status",
        &["session", "status"],
        &["pkg", "session"],
        &[],
        READ_ONLY,
    ),
    route(
        "session-test",
        &["session", "test"],
        &["pkg", "session", "caps"],
        &[],
        IDEMPOTENT_WRITE,
    ),
    route("package", &["pack"], &["pkg"], &[], IDEMPOTENT_WRITE),
    route(
        "build",
        &["pkg", "build"],
        &["caps", "log", "pkg", "target", "out_dir"],
        &[],
        IDEMPOTENT_WRITE,
    ),
];

const fn route(
    name: &'static str,
    route: &'static [&'static str],
    allowed: &'static [&'static str],
    forced_required: &'static [&'static str],
    annotations: ToolAnnotations,
) -> RoutePolicy {
    RoutePolicy {
        name,
        route,
        allowed,
        forced_required,
        annotations,
    }
}

pub(super) fn bindings(profile: RuntimeProfile) -> Result<Vec<ToolBinding>, String> {
    let root = Cli::command();
    let mut tools = ROUTES
        .iter()
        .map(|policy| binding(&root, *policy, profile))
        .collect::<Result<Vec<_>, _>>()?;
    tools.sort_by(|left, right| left.name.cmp(right.name));
    let mut names = BTreeSet::new();
    for tool in &tools {
        if !names.insert(tool.name) {
            return Err(format!("duplicate MCP tool name `{}`", tool.name));
        }
    }
    Ok(tools)
}

fn binding(
    root: &Command,
    policy: RoutePolicy,
    profile: RuntimeProfile,
) -> Result<ToolBinding, String> {
    let allowed = policy.allowed.iter().copied().collect::<BTreeSet<_>>();
    let forced = policy
        .forced_required
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut found = BTreeSet::new();
    let mut arguments = Vec::new();
    let mut command = root;
    let mut leaf_about = None;
    for (depth, segment) in policy.route.iter().enumerate() {
        command = command
            .get_subcommands()
            .find(|candidate| candidate.get_name() == *segment)
            .ok_or_else(|| format!("MCP route `{}` has no CLI command `{segment}`", policy.name))?;
        leaf_about = command.get_about().map(ToString::to_string);
        for arg in command
            .get_arguments()
            .filter(|arg| !arg.is_hide_set() && !arg.is_global_set() && !is_internal(arg))
        {
            let id = arg.get_id().as_str();
            if !allowed.contains(id) {
                continue;
            }
            if !found.insert(id.to_string()) {
                return Err(format!(
                    "MCP route `{}` exposes duplicate CLI argument `{id}`",
                    policy.name
                ));
            }
            arguments.push(arg_binding(arg, depth, forced.contains(id), profile));
        }
    }
    let missing = allowed
        .iter()
        .filter(|id| !found.contains(**id))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "MCP route `{}` references missing CLI arguments: {}",
            policy.name,
            missing.join(", ")
        ));
    }
    for (depth, segment) in policy.route.iter().enumerate() {
        let mut at = root;
        for child in &policy.route[..=depth] {
            at = at
                .get_subcommands()
                .find(|candidate| candidate.get_name() == *child)
                .ok_or_else(|| format!("MCP route segment `{segment}` disappeared"))?;
        }
        for arg in at
            .get_arguments()
            .filter(|arg| arg.is_required_set() && !arg.is_global_set() && !is_internal(arg))
        {
            if !allowed.contains(arg.get_id().as_str()) {
                return Err(format!(
                    "MCP route `{}` omits required CLI argument `{}`",
                    policy.name,
                    arg.get_id()
                ));
            }
        }
    }
    arguments.sort_by(|left, right| {
        (
            left.depth,
            !left.positional,
            left.position,
            left.id.as_str(),
        )
            .cmp(&(
                right.depth,
                !right.positional,
                right.position,
                right.id.as_str(),
            ))
    });
    let mut properties = Map::new();
    properties.insert(
        "root".to_string(),
        json!({
            "type": "string",
            "description": "Optional exact file:// workspace root URI returned by roots/list."
        }),
    );
    let mut required = Vec::new();
    for argument in &arguments {
        properties.insert(argument.id.clone(), argument.schema.clone());
        if argument.required {
            required.push(argument.id.clone());
        }
    }
    required.sort();
    let input_schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    });
    let definition = json!({
        "name": policy.name,
        "title": leaf_about.clone().unwrap_or_else(|| policy.name.to_string()),
        "description": leaf_about.unwrap_or_else(|| policy.name.to_string()),
        "inputSchema": input_schema,
        "outputSchema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["ok", "kind"],
            "properties": {"ok": {"type": "boolean"}, "kind": {"type": "string"}},
            "additionalProperties": true
        },
        "annotations": {
            "readOnlyHint": policy.annotations.read_only,
            "destructiveHint": policy.annotations.destructive,
            "idempotentHint": policy.annotations.idempotent,
            "openWorldHint": policy.annotations.open_world
        },
        "execution": {"taskSupport": "forbidden"},
        "_meta": {
            "io.genesiscode/cliRoute": policy.route,
            "io.genesiscode/interfaceSchema": MCP_INTERFACE_SCHEMA
        }
    });
    Ok(ToolBinding {
        name: policy.name,
        route: policy.route.to_vec(),
        arguments,
        definition,
    })
}

fn arg_binding(
    arg: &Arg,
    depth: usize,
    forced_required: bool,
    profile: RuntimeProfile,
) -> ArgBinding {
    let kind = value_kind(arg);
    let mut schema = Map::new();
    match kind {
        ValueKind::Bool => {
            schema.insert("type".to_string(), json!("boolean"));
        }
        ValueKind::Integer => {
            schema.insert("type".to_string(), json!("integer"));
        }
        ValueKind::Number => {
            schema.insert("type".to_string(), json!("number"));
        }
        ValueKind::String => {
            schema.insert("type".to_string(), json!("string"));
        }
        ValueKind::StringArray => {
            schema.insert("type".to_string(), json!("array"));
            schema.insert("items".to_string(), json!({"type": "string"}));
        }
    }
    if let Some(help) = arg.get_help() {
        schema.insert("description".to_string(), json!(help.to_string()));
    }
    let allowed = possible_values(arg, profile);
    if !allowed.is_empty() {
        schema.insert("enum".to_string(), json!(allowed));
    }
    let defaults = arg
        .get_default_values()
        .iter()
        .map(|value| value.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if defaults.len() == 1 {
        schema.insert("default".to_string(), scalar_default(&defaults[0], kind));
    } else if !defaults.is_empty() {
        schema.insert("default".to_string(), json!(defaults));
    }
    ArgBinding {
        id: arg.get_id().to_string(),
        long: arg.get_long().map(str::to_string),
        positional: arg.is_positional(),
        position: arg.get_index(),
        depth,
        required: forced_required || arg.is_required_set(),
        kind,
        bool_flag_value: !matches!(arg.get_action(), ArgAction::SetFalse),
        schema: Value::Object(schema),
    }
}

fn value_kind(arg: &Arg) -> ValueKind {
    match arg.get_action() {
        ArgAction::SetTrue | ArgAction::SetFalse => ValueKind::Bool,
        ArgAction::Count => ValueKind::Integer,
        ArgAction::Append => ValueKind::StringArray,
        _ => {
            let id = arg.get_value_parser().type_id();
            if [
                TypeId::of::<u8>(),
                TypeId::of::<u16>(),
                TypeId::of::<u32>(),
                TypeId::of::<u64>(),
                TypeId::of::<usize>(),
                TypeId::of::<i8>(),
                TypeId::of::<i16>(),
                TypeId::of::<i32>(),
                TypeId::of::<i64>(),
                TypeId::of::<isize>(),
            ]
            .iter()
            .any(|expected| id == *expected)
            {
                ValueKind::Integer
            } else if id == TypeId::of::<f32>() || id == TypeId::of::<f64>() {
                ValueKind::Number
            } else {
                ValueKind::String
            }
        }
    }
}

fn possible_values(arg: &Arg, profile: RuntimeProfile) -> Vec<String> {
    let mut values = arg
        .get_possible_values()
        .into_iter()
        .map(|value| value.get_name().to_string())
        .collect::<Vec<_>>();
    if values.is_empty() && matches!(arg.get_long(), Some("engine")) {
        values = match profile {
            RuntimeProfile::Production => vec!["selfhost".to_string()],
            #[cfg(feature = "parity-harness")]
            RuntimeProfile::ParityHarness => vec!["selfhost".to_string(), "rust".to_string()],
        };
    }
    values
}

fn scalar_default(value: &str, kind: ValueKind) -> Value {
    match kind {
        ValueKind::Bool => value
            .parse::<bool>()
            .map(Value::Bool)
            .unwrap_or_else(|_| json!(value)),
        ValueKind::Integer => value
            .parse::<i64>()
            .map(Into::into)
            .unwrap_or_else(|_| json!(value)),
        ValueKind::Number => value
            .parse::<f64>()
            .map(Into::into)
            .unwrap_or_else(|_| json!(value)),
        ValueKind::String | ValueKind::StringArray => json!(value),
    }
}

fn is_internal(arg: &Arg) -> bool {
    matches!(arg.get_id().as_str(), "help" | "version")
}

pub(crate) fn interface_manifest(profile: RuntimeProfile) -> Result<Value, String> {
    let tools = bindings(profile)?;
    let mut manifest = json!({
        "schema": MCP_INTERFACE_SCHEMA,
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "transport": "stdio-newline-delimited-jsonrpc-2.0",
        "runtimeProfile": cli_schema::runtime_profile_token(profile),
        "tasks": {"advertised": false, "status": "experimental-unnegotiated"},
        "tools": tools.iter().map(|tool| tool.definition.clone()).collect::<Vec<_>>(),
        "resources": super::resources::resource_definitions(),
    });
    let canonical = json_canonical_string(&manifest);
    let identity = format!("{:x}", Sha256::digest(canonical.as_bytes()));
    manifest
        .as_object_mut()
        .ok_or_else(|| "MCP interface manifest is not an object".to_string())?
        .insert("identitySha256".to_string(), json!(identity));
    Ok(manifest)
}

pub(super) fn tool_argv(
    tool: &ToolBinding,
    arguments: &Map<String, Value>,
) -> Result<Vec<String>, String> {
    let allowed = tool
        .arguments
        .iter()
        .map(|argument| argument.id.as_str())
        .chain(std::iter::once("root"))
        .collect::<BTreeSet<_>>();
    if let Some(unknown) = arguments.keys().find(|key| !allowed.contains(key.as_str())) {
        return Err(format!(
            "unknown argument `{unknown}` for tool `{}`",
            tool.name
        ));
    }
    for argument in &tool.arguments {
        if argument.required && !arguments.contains_key(&argument.id) {
            return Err(format!("missing required argument `{}`", argument.id));
        }
    }
    let mut argv = Vec::new();
    for depth in 0..tool.route.len() {
        argv.push(tool.route[depth].to_string());
        for binding in tool
            .arguments
            .iter()
            .filter(|argument| argument.depth == depth)
        {
            let Some(value) = arguments.get(&binding.id) else {
                continue;
            };
            append_argument(&mut argv, binding, value)?;
        }
    }
    Ok(argv)
}

fn append_argument(
    argv: &mut Vec<String>,
    binding: &ArgBinding,
    value: &Value,
) -> Result<(), String> {
    let flag = binding.long.as_ref().map(|long| format!("--{long}"));
    match binding.kind {
        ValueKind::Bool => {
            let enabled = value
                .as_bool()
                .ok_or_else(|| format!("argument `{}` must be boolean", binding.id))?;
            if enabled == binding.bool_flag_value {
                let flag =
                    flag.ok_or_else(|| format!("boolean `{}` has no CLI flag", binding.id))?;
                argv.push(flag);
            }
        }
        ValueKind::Integer => {
            let scalar = value
                .as_i64()
                .map(|value| value.to_string())
                .or_else(|| value.as_u64().map(|value| value.to_string()))
                .ok_or_else(|| format!("argument `{}` must be an integer", binding.id))?;
            append_scalar(argv, binding, flag, scalar)?;
        }
        ValueKind::Number => {
            let scalar = value
                .as_f64()
                .map(|value| value.to_string())
                .ok_or_else(|| format!("argument `{}` must be a number", binding.id))?;
            append_scalar(argv, binding, flag, scalar)?;
        }
        ValueKind::String => {
            let scalar = value
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("argument `{}` must be a non-empty string", binding.id))?
                .to_string();
            append_scalar(argv, binding, flag, scalar)?;
        }
        ValueKind::StringArray => {
            let values = value
                .as_array()
                .ok_or_else(|| format!("argument `{}` must be a string array", binding.id))?;
            for value in values {
                let scalar = value
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        format!("argument `{}` must contain non-empty strings", binding.id)
                    })?
                    .to_string();
                append_scalar(argv, binding, flag.clone(), scalar)?;
            }
        }
    }
    Ok(())
}

fn append_scalar(
    argv: &mut Vec<String>,
    binding: &ArgBinding,
    flag: Option<String>,
    value: String,
) -> Result<(), String> {
    if binding.positional {
        argv.push(value);
    } else {
        argv.push(flag.ok_or_else(|| format!("argument `{}` has no CLI spelling", binding.id))?);
        argv.push(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_routes_are_clap_derived_and_tasks_are_forbidden() {
        let tools = bindings(RuntimeProfile::Production).expect("catalog");
        assert_eq!(tools.len(), 20);
        for tool in tools {
            assert_eq!(tool.definition["execution"]["taskSupport"], "forbidden");
            assert_eq!(
                tool.definition["inputSchema"]["additionalProperties"],
                false
            );
        }
    }

    #[test]
    fn argv_generation_places_parent_options_before_nested_command() {
        let tools = bindings(RuntimeProfile::Production).expect("catalog");
        let build = tools
            .iter()
            .find(|tool| tool.name == "build")
            .expect("build");
        let args = serde_json::from_value::<Map<String, Value>>(json!({
            "caps": "caps.toml",
            "target": "web"
        }))
        .expect("args");
        let argv = tool_argv(build, &args).expect("argv");
        assert_eq!(
            argv,
            ["pkg", "--caps", "caps.toml", "build", "--target", "web"]
        );
    }
}
