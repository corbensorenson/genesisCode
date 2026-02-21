use super::*;
use clap::{Arg, Command, CommandFactory};

pub(super) fn cmd_cli_schema(cli: &Cli) -> Result<CmdOut, CliError> {
    let profile = runtime_profile();
    let command = build_cli_schema(profile);
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/cli-schema-v0.1",
        data: Some(serde_json::json!({
            "schema": "genesis/cli-schema-v0.1",
            "runtime_profile": runtime_profile_token(profile),
            "command": command,
        })),
        error: None,
    };
    let json = json_envelope_value(env)?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}

pub(super) fn build_cli_schema(profile: RuntimeProfile) -> serde_json::Value {
    let root = Cli::command();
    command_schema(&root, profile, &[])
}

pub(super) fn runtime_profile_token(profile: RuntimeProfile) -> &'static str {
    match profile {
        RuntimeProfile::Production => "production",
        #[cfg(feature = "parity-harness")]
        RuntimeProfile::ParityHarness => "parity-harness",
    }
}

fn command_schema(cmd: &Command, profile: RuntimeProfile, parent: &[String]) -> serde_json::Value {
    let mut path = parent.to_vec();
    path.push(cmd.get_name().to_string());

    let mut options: Vec<serde_json::Value> = cmd
        .get_arguments()
        .filter(|arg| !arg.is_hide_set() && !is_clap_internal_arg(arg))
        .map(|arg| arg_schema(arg, profile))
        .collect();
    options.sort_by(|a, b| {
        let ka = a
            .get("long")
            .and_then(|v| v.as_str())
            .or_else(|| a.get("name").and_then(|v| v.as_str()))
            .unwrap_or("");
        let kb = b
            .get("long")
            .and_then(|v| v.as_str())
            .or_else(|| b.get("name").and_then(|v| v.as_str()))
            .unwrap_or("");
        ka.cmp(kb)
    });

    let mut subcommands: Vec<serde_json::Value> = cmd
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .map(|sub| command_schema(sub, profile, &path))
        .collect();
    subcommands.sort_by(|a, b| {
        let ka = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let kb = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        ka.cmp(kb)
    });

    serde_json::json!({
        "name": cmd.get_name(),
        "path": path,
        "about": cmd.get_about().map(|v| v.to_string()),
        "options": options,
        "subcommands": subcommands,
    })
}

fn arg_schema(arg: &Arg, profile: RuntimeProfile) -> serde_json::Value {
    let long = arg.get_long().map(str::to_string);
    let mut allowed_values = collect_possible_values(arg);
    if allowed_values.is_empty()
        && let Some(long_name) = arg.get_long()
        && let Some(expected) = expected_values_by_profile(long_name, profile)
    {
        allowed_values = expected;
    }
    let default_values = arg
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let value_names = arg
        .get_value_names()
        .map(|names| names.iter().map(|v| v.to_string()).collect::<Vec<_>>())
        .unwrap_or_default();

    serde_json::json!({
        "name": arg.get_id().to_string(),
        "long": long,
        "short": arg.get_short().map(|v| v.to_string()),
        "help": arg.get_help().map(|v| v.to_string()),
        "required": arg.is_required_set(),
        "global": arg.is_global_set(),
        "positional": arg.is_positional(),
        "value_names": value_names,
        "default_values": default_values,
        "allowed_values": allowed_values,
    })
}

fn collect_possible_values(arg: &Arg) -> Vec<String> {
    arg.get_possible_values()
        .into_iter()
        .map(|v| v.get_name().to_string())
        .collect()
}

fn expected_values_by_profile(long_name: &str, profile: RuntimeProfile) -> Option<Vec<String>> {
    if long_name == "engine" || long_name == "coreform-frontend" {
        return Some(match profile {
            RuntimeProfile::Production => vec!["selfhost".to_string()],
            #[cfg(feature = "parity-harness")]
            RuntimeProfile::ParityHarness => vec!["selfhost".to_string(), "rust".to_string()],
        });
    }
    None
}

fn is_clap_internal_arg(arg: &Arg) -> bool {
    let id = arg.get_id().as_str();
    id == "help" || id == "version"
}
