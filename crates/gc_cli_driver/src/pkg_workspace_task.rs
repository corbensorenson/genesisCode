use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, Value};
use gc_pkg::{WorkspaceConfig, WorkspaceTask};

use crate::pkg_task_runner::WorkspaceTaskAction;

const AUTHORITY_BINDING: &str = "core/pkg::workspace-task-authority";
const REQUEST_KIND: &str = "genesis/pkg-workspace-task-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-workspace-task-authority-result-v0.1";
const TASK_LIMIT: usize = 256;
const TASK_NAME_LIMIT: usize = 256;
const COMMAND_LIMIT: usize = 64;
const TASK_ARG_LIMIT: usize = 64;
const TASK_STRING_LIMIT: usize = 4096;

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve(
    cli: &crate::Cli,
    workspace_file: &Path,
    workspace: &WorkspaceConfig,
    default_runtime_backend: Option<&str>,
    profile_runtime_backend: Option<&str>,
    active_runtime_backend: &str,
    task_name: &str,
) -> Result<WorkspaceTaskAction, String> {
    let request = task_request(
        workspace,
        default_runtime_backend,
        profile_runtime_backend,
        active_runtime_backend,
        task_name,
    )?;
    let request_hash = hex32(hash_term(&request));
    let engines = task_engine_inventory();
    let mut context = crate::mk_ctx(cli);
    let prelude = crate::build_prelude(&mut context);
    let mut environment = prelude.env;
    crate::load_selfhost_toolchain(cli, &mut context, &mut environment)
        .map_err(|error| format!("load workspace-task authority: {error:?}"))?;
    let authority = environment
        .get(AUTHORITY_BINDING)
        .ok_or_else(|| format!("missing binding {AUTHORITY_BINDING}"))?;
    let value = authority
        .apply(&mut context, Value::data(request))
        .map_err(|error| format!("{AUTHORITY_BINDING} failed: {error}"))?;
    if let Some((code, message, _)) = crate::extract_protocol_error(&context, &value) {
        return Err(format!(
            "{AUTHORITY_BINDING} returned sealed error: {code}: {message}"
        ));
    }
    decode_authorized(
        value,
        workspace_file,
        task_name,
        active_runtime_backend,
        &engines,
        &request_hash,
    )
}

fn task_request(
    workspace: &WorkspaceConfig,
    default_runtime_backend: Option<&str>,
    profile_runtime_backend: Option<&str>,
    active_runtime_backend: &str,
    task_name: &str,
) -> Result<Term, String> {
    bounded(task_name, TASK_NAME_LIMIT, "workspace task name")?;
    bounded(
        active_runtime_backend,
        COMMAND_LIMIT,
        "active runtime backend",
    )?;
    optional_bounded(
        default_runtime_backend,
        COMMAND_LIMIT,
        "default runtime backend",
    )?;
    optional_bounded(
        profile_runtime_backend,
        COMMAND_LIMIT,
        "profile runtime backend",
    )?;
    if workspace.tasks.len() > TASK_LIMIT {
        return Err(format!(
            "workspace task inventory exceeds transport limit {TASK_LIMIT}"
        ));
    }
    let tasks = workspace
        .tasks
        .iter()
        .map(|(name, task)| task_observation(name, task))
        .collect::<Result<Vec<_>, _>>()?;
    let engines = task_engine_inventory()
        .into_iter()
        .map(|engine| Term::Str(engine.to_string()))
        .collect();
    Ok(map([
        (":active", Term::Str(active_runtime_backend.to_string())),
        (":default", optional_string(default_runtime_backend)),
        (":engines", Term::Vector(engines)),
        (":kind", Term::Str(REQUEST_KIND.to_string())),
        (":profile", Term::Str("dev".to_string())),
        (":profile-backend", optional_string(profile_runtime_backend)),
        (":task", Term::Str(task_name.to_string())),
        (":tasks", Term::Vector(tasks)),
        (":v", Term::Int(1.into())),
    ]))
}

fn task_observation(name: &str, task: &WorkspaceTask) -> Result<Term, String> {
    bounded(name, TASK_NAME_LIMIT, "workspace task name")?;
    bounded(&task.cmd, COMMAND_LIMIT, "workspace task command")?;
    optional_bounded(
        task.file.as_deref(),
        TASK_STRING_LIMIT,
        "workspace task file",
    )?;
    optional_bounded(
        task.pkg.as_deref(),
        TASK_STRING_LIMIT,
        "workspace task package",
    )?;
    if task.args.len() > TASK_ARG_LIMIT {
        return Err(format!(
            "workspace task `{name}` exceeds argument transport limit {TASK_ARG_LIMIT}"
        ));
    }
    for argument in &task.args {
        bounded(argument, TASK_STRING_LIMIT, "workspace task argument")?;
    }
    Ok(map([
        (
            ":args",
            Term::Vector(task.args.iter().cloned().map(Term::Str).collect()),
        ),
        (":cmd", Term::Str(task.cmd.clone())),
        (":file", optional_string(task.file.as_deref())),
        (":name", Term::Str(name.to_string())),
        (":pkg", optional_string(task.pkg.as_deref())),
    ]))
}

fn decode_authorized(
    value: Value,
    workspace_file: &Path,
    requested_task: &str,
    active_runtime_backend: &str,
    engines: &[&str],
    request_hash: &str,
) -> Result<WorkspaceTaskAction, String> {
    let Some(Term::Map(envelope)) = value.to_plain_term() else {
        return Err("workspace-task authority returned non-map".to_string());
    };
    require_exact_fields(
        &envelope,
        &[
            ":code",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
            ":value",
        ],
        "workspace-task envelope",
    )?;
    require_string(&envelope, ":kind", RESULT_KIND)?;
    require_string(&envelope, ":request-h", request_hash)?;
    require_int(&envelope, ":v", 1)?;
    match field(&envelope, ":ok")? {
        Term::Bool(false) => {
            require_nil(&envelope, ":value")?;
            let code = required_string(&envelope, ":code")?;
            if !matches!(
                code,
                "core/pkg/bad-workspace-task" | "core/pkg/bad-workspace-env-selection"
            ) {
                return Err("workspace-task authority returned invalid rejection code".to_string());
            }
            return Err(format!(
                "{code}: {}",
                required_string(&envelope, ":message")?
            ));
        }
        Term::Bool(true) => {
            require_nil(&envelope, ":code")?;
            require_nil(&envelope, ":message")?;
        }
        _ => return Err("workspace-task envelope :ok must be bool".to_string()),
    }
    let Term::Map(result) = field(&envelope, ":value")? else {
        return Err("workspace-task result :value must be map".to_string());
    };
    require_exact_fields(
        result,
        &[":action", ":active", ":compatible", ":selected", ":source"],
        "workspace-task result",
    )?;
    require_string(result, ":active", active_runtime_backend)?;
    require_bool(result, ":compatible", true)?;
    let selected = required_string(result, ":selected")?;
    if !matches!(selected, "headless" | "gpu" | "gfx" | "backend") {
        return Err("workspace-task authority returned invalid backend".to_string());
    }
    match field(result, ":source")? {
        Term::Symbol(source) if matches!(source.as_str(), ":profile" | ":default" | ":builtin") => {
        }
        _ => return Err("workspace-task authority returned invalid backend source".to_string()),
    }
    let Term::Map(action) = field(result, ":action")? else {
        return Err("workspace-task result :action must be map".to_string());
    };
    decode_action(action, workspace_file, requested_task, engines)
}

fn decode_action(
    action: &BTreeMap<TermOrdKey, Term>,
    workspace_file: &Path,
    requested_task: &str,
    engines: &[&str],
) -> Result<WorkspaceTaskAction, String> {
    require_exact_fields(
        action,
        &[
            ":action",
            ":caps",
            ":check",
            ":contract-h",
            ":emit-wasm",
            ":engine",
            ":file",
            ":log",
            ":out",
            ":pkg",
            ":stage1-gate",
            ":stage1-pipeline",
            ":stage2-gate",
            ":task",
        ],
        "workspace-task action",
    )?;
    require_string(action, ":task", requested_task)?;
    let kind = required_string(action, ":action")?;
    let caps = optional_path(action, ":caps", workspace_file)?;
    let contract_hash_hex = optional_field_string(action, ":contract-h")?;
    let emit_wasm = optional_path(action, ":emit-wasm", workspace_file)?;
    let engine = optional_field_string(action, ":engine")?;
    if let Some(engine) = engine.as_deref()
        && !engines.contains(&engine)
    {
        return Err("workspace-task authority returned unavailable engine".to_string());
    }
    let file = optional_path(action, ":file", workspace_file)?;
    let log = optional_path(action, ":log", workspace_file)?;
    let out = optional_path(action, ":out", workspace_file)?;
    let pkg = optional_path(action, ":pkg", workspace_file)?;
    let check = bool_field(action, ":check")?;
    let stage1_gate = bool_field(action, ":stage1-gate")?;
    let stage1_pipeline = bool_field(action, ":stage1-pipeline")?;
    let stage2_gate = bool_field(action, ":stage2-gate")?;

    match kind {
        "test" => {
            require_absent_action_fields(
                contract_hash_hex.as_deref(),
                emit_wasm.as_ref(),
                engine.as_deref(),
                file.as_ref(),
                log.as_ref(),
                out.as_ref(),
                check || stage1_gate || stage1_pipeline || stage2_gate,
            )?;
            Ok(WorkspaceTaskAction::Test {
                pkg: require_path(pkg, ":pkg")?,
                caps,
            })
        }
        "pack" | "typecheck" => {
            require_absent_action_fields(
                contract_hash_hex.as_deref(),
                emit_wasm.as_ref(),
                engine.as_deref(),
                file.as_ref(),
                log.as_ref(),
                out.as_ref(),
                check || stage1_gate || stage1_pipeline || stage2_gate || caps.is_some(),
            )?;
            let pkg = require_path(pkg, ":pkg")?;
            if kind == "pack" {
                Ok(WorkspaceTaskAction::Pack { pkg })
            } else {
                Ok(WorkspaceTaskAction::Typecheck { pkg })
            }
        }
        "run" => {
            require_absent_action_fields(
                contract_hash_hex.as_deref(),
                emit_wasm.as_ref(),
                None,
                None,
                None,
                out.as_ref(),
                check || stage1_gate || stage1_pipeline || stage2_gate || pkg.is_some(),
            )?;
            Ok(WorkspaceTaskAction::Run {
                file: require_path(file, ":file")?,
                caps,
                log,
                engine,
            })
        }
        "contract" => {
            require_absent_action_fields(
                None,
                emit_wasm.as_ref(),
                None,
                None,
                None,
                out.as_ref(),
                check || stage1_gate || stage1_pipeline || stage2_gate || pkg.is_some(),
            )?;
            let hash = contract_hash_hex
                .ok_or_else(|| "workspace-task contract action missing :contract-h".to_string())?;
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("workspace-task authority returned invalid contract hash".to_string());
            }
            Ok(WorkspaceTaskAction::Contract {
                file: require_path(file, ":file")?,
                caps,
                log,
                engine,
                contract_hash_hex: hash,
            })
        }
        "eval" => {
            require_absent_action_fields(
                contract_hash_hex.as_deref(),
                emit_wasm.as_ref(),
                None,
                None,
                log.as_ref(),
                out.as_ref(),
                check || caps.is_some() || pkg.is_some(),
            )?;
            Ok(WorkspaceTaskAction::Eval {
                file: require_path(file, ":file")?,
                engine,
                stage1_pipeline,
                stage1_gate,
                stage2_gate,
            })
        }
        "fmt" => {
            require_absent_action_fields(
                contract_hash_hex.as_deref(),
                emit_wasm.as_ref(),
                None,
                None,
                log.as_ref(),
                out.as_ref(),
                stage1_gate || stage1_pipeline || stage2_gate || caps.is_some() || pkg.is_some(),
            )?;
            Ok(WorkspaceTaskAction::Fmt {
                file: require_path(file, ":file")?,
                check,
                engine,
            })
        }
        "optimize" => {
            require_absent_action_fields(
                contract_hash_hex.as_deref(),
                None,
                None,
                None,
                log.as_ref(),
                None,
                check || stage1_pipeline || caps.is_some() || pkg.is_some(),
            )?;
            Ok(WorkspaceTaskAction::Optimize {
                file: require_path(file, ":file")?,
                out,
                emit_wasm,
                engine,
                stage1_gate,
                stage2_gate,
            })
        }
        _ => Err("workspace-task authority returned invalid action".to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn require_absent_action_fields(
    contract_hash: Option<&str>,
    emit_wasm: Option<&PathBuf>,
    engine: Option<&str>,
    file: Option<&PathBuf>,
    log: Option<&PathBuf>,
    out: Option<&PathBuf>,
    unexpected_flag: bool,
) -> Result<(), String> {
    if contract_hash.is_none()
        && emit_wasm.is_none()
        && engine.is_none()
        && file.is_none()
        && log.is_none()
        && out.is_none()
        && !unexpected_flag
    {
        Ok(())
    } else {
        Err("workspace-task authority returned contradictory action fields".to_string())
    }
}

fn optional_path(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    workspace_file: &Path,
) -> Result<Option<PathBuf>, String> {
    optional_field_string(fields, name)
        .map(|value| value.map(|raw| resolve_workspace_relative_path(workspace_file, &raw)))
}

fn optional_field_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, String> {
    match field(fields, name)? {
        Term::Nil => Ok(None),
        Term::Str(value) if !value.is_empty() && value.chars().count() <= TASK_STRING_LIMIT => {
            Ok(Some(value.clone()))
        }
        _ => Err(format!(
            "workspace-task action {name} must be bounded string or nil"
        )),
    }
}

fn resolve_workspace_relative_path(workspace_file: &Path, raw: &str) -> PathBuf {
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        return candidate;
    }
    workspace_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(candidate)
}

fn require_path(value: Option<PathBuf>, name: &str) -> Result<PathBuf, String> {
    value.ok_or_else(|| format!("workspace-task action requires {name}"))
}

fn bool_field(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<bool, String> {
    match field(fields, name)? {
        Term::Bool(value) => Ok(*value),
        _ => Err(format!("workspace-task action {name} must be bool")),
    }
}

fn require_bool(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: bool,
) -> Result<(), String> {
    if bool_field(fields, name)? == expected {
        Ok(())
    } else {
        Err(format!("workspace-task {name} contradicts request"))
    }
}

fn task_engine_inventory() -> Vec<&'static str> {
    if cfg!(feature = "parity-harness") {
        vec!["selfhost", "rust"]
    } else {
        vec!["selfhost"]
    }
}

fn bounded(value: &str, limit: usize, label: &str) -> Result<(), String> {
    if !value.is_empty() && value.chars().count() <= limit {
        Ok(())
    } else {
        Err(format!("{label} must contain 1..={limit} characters"))
    }
}

fn optional_bounded(value: Option<&str>, limit: usize, label: &str) -> Result<(), String> {
    if let Some(value) = value
        && value.chars().count() > limit
    {
        return Err(format!("{label} exceeds transport limit {limit}"));
    }
    Ok(())
}

fn optional_string(value: Option<&str>) -> Term {
    value
        .map(|value| Term::Str(value.to_string()))
        .unwrap_or(Term::Nil)
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn require_exact_fields(
    fields: &BTreeMap<TermOrdKey, Term>,
    names: &[&str],
    label: &str,
) -> Result<(), String> {
    let expected = names
        .iter()
        .map(|name| TermOrdKey(Term::symbol(*name)))
        .collect::<BTreeSet<_>>();
    if fields.keys().cloned().collect::<BTreeSet<_>>() == expected {
        Ok(())
    } else {
        Err(format!("{label} field set mismatch"))
    }
}

fn field<'a>(fields: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> Result<&'a Term, String> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| format!("workspace-task result missing {name}"))
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, String> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(format!("workspace-task {name} must be string")),
    }
}

fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), String> {
    if required_string(fields, name)? == expected {
        Ok(())
    } else {
        Err(format!("workspace-task {name} contradicts request"))
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Int(value) if value == &expected.into() => Ok(()),
        _ => Err(format!("workspace-task {name} must be {expected}")),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), String> {
    if field(fields, name)? == &Term::Nil {
        Ok(())
    } else {
        Err(format!("workspace-task {name} must be nil"))
    }
}

fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
