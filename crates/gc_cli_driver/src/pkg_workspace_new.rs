use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, Value};
use gc_pkg::{GenesisLock, WorkspaceConfig};

use super::LocalPkgResult;

const AUTHORITY_BINDING: &str = "core/pkg::workspace-new-authority";
const REQUEST_KIND: &str = "genesis/pkg-workspace-new-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-workspace-new-authority-result-v0.1";

struct AuthorizedWorkspaceNew {
    lock_body: String,
    workspace_body: String,
    report: Term,
}

pub(super) fn handle_new(
    cli: &crate::Cli,
    workspace: &str,
    lock: &Path,
    workspace_file: &Path,
    policy: &str,
    registry_default: Option<&str>,
    members: &[String],
) -> Result<LocalPkgResult, String> {
    let active_backend = crate::active_runtime_backend_profile();
    let request = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":active-backend")),
                Term::Str(active_backend.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(REQUEST_KIND.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":members")),
                Term::Vector(members.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":policy")),
                Term::Str(policy.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":registry-default")),
                registry_default
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(workspace.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":workspace-file")),
                Term::Str(workspace_file.display().to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let request_hash = hex32(hash_term(&request));
    let mut context = crate::mk_ctx(cli);
    let prelude = crate::build_prelude(&mut context);
    let mut environment = prelude.env;
    crate::load_selfhost_toolchain(cli, &mut context, &mut environment)
        .map_err(|error| format!("load workspace-new authority: {error:?}"))?;
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
    let authorized = decode_authorized(
        value,
        &request_hash,
        workspace,
        lock,
        workspace_file,
        policy,
        registry_default,
        active_backend,
    )?;

    preflight_paths(lock, workspace_file)?;
    crate::pkg_scaffold::atomic_write_text(lock, authorized.lock_body.as_bytes())
        .map_err(|error| format!("write {}: {error}", lock.display()))?;
    crate::pkg_scaffold::atomic_write_text(workspace_file, authorized.workspace_body.as_bytes())
        .map_err(|error| format!("write {}: {error}", workspace_file.display()))?;

    Ok(LocalPkgResult {
        kind: "genesis/pkg-new-v0.1",
        log_op: "pkg-new",
        program_hash: hash_term(&authorized.report),
        value: authorized.report,
    })
}

#[allow(clippy::too_many_arguments)]
fn decode_authorized(
    value: Value,
    request_hash: &str,
    requested_workspace: &str,
    lock_path: &Path,
    workspace_path: &Path,
    requested_policy: &str,
    requested_registry: Option<&str>,
    active_backend: &str,
) -> Result<AuthorizedWorkspaceNew, String> {
    let Some(Term::Map(envelope)) = value.to_plain_term() else {
        return Err("workspace-new authority returned non-map".to_string());
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
        "workspace-new envelope",
    )?;
    require_string(&envelope, ":kind", RESULT_KIND)?;
    require_int(&envelope, ":v", 1)?;
    require_string(&envelope, ":request-h", request_hash)?;
    match field(&envelope, ":ok")? {
        Term::Bool(false) => {
            require_nil(&envelope, ":value")?;
            require_string(&envelope, ":code", "core/pkg/bad-workspace-new")?;
            return Err(format!(
                "core/pkg/bad-workspace-new: {}",
                required_string(&envelope, ":message")?
            ));
        }
        Term::Bool(true) => {
            require_nil(&envelope, ":code")?;
            require_nil(&envelope, ":message")?;
        }
        _ => return Err("workspace-new envelope :ok must be bool".to_string()),
    }
    let Term::Map(result) = field(&envelope, ":value")? else {
        return Err("workspace-new result :value must be map".to_string());
    };
    require_exact_fields(result, &[":files", ":report"], "workspace-new result")?;
    let Term::Vector(files) = field(result, ":files")? else {
        return Err("workspace-new result :files must be vector".to_string());
    };
    if files.len() != 2 {
        return Err("workspace-new result must contain two files".to_string());
    }
    let expected_paths = [
        lock_path.display().to_string(),
        workspace_path.display().to_string(),
    ];
    let mut bodies = Vec::with_capacity(2);
    let mut hashes = Vec::with_capacity(2);
    for (index, item) in files.iter().enumerate() {
        let Term::Map(file) = item else {
            return Err("workspace-new file must be map".to_string());
        };
        require_exact_fields(file, &[":body", ":h", ":path"], "workspace-new file")?;
        require_string(file, ":path", &expected_paths[index])?;
        let body = required_string(file, ":body")?.to_string();
        let actual_hash = blake3::hash(body.as_bytes()).to_hex().to_string();
        require_string(file, ":h", &actual_hash)?;
        bodies.push(body);
        hashes.push(actual_hash);
    }

    let lock = GenesisLock::from_toml_str(lock_path, &bodies[0])
        .map_err(|error| format!("workspace-new lock document is invalid: {error}"))?;
    let workspace = WorkspaceConfig::from_toml_str(workspace_path, &bodies[1])
        .map_err(|error| format!("workspace-new workspace document is invalid: {error}"))?;
    validate_documents(
        &lock,
        &workspace,
        requested_workspace,
        requested_policy,
        requested_registry,
        active_backend,
    )?;

    let report = field(result, ":report")?.clone();
    let Term::Map(report_fields) = &report else {
        return Err("workspace-new report must be map".to_string());
    };
    require_exact_fields(
        report_fields,
        &[
            ":lock",
            ":lock-h",
            ":members",
            ":ok",
            ":workspace",
            ":workspace-file",
            ":workspace-h",
        ],
        "workspace-new report",
    )?;
    if field(report_fields, ":ok")? != &Term::Bool(true) {
        return Err("workspace-new report :ok must be true".to_string());
    }
    require_string(report_fields, ":workspace", requested_workspace)?;
    require_string(report_fields, ":workspace-file", &expected_paths[1])?;
    require_string(report_fields, ":workspace-h", &hashes[1])?;
    require_int(report_fields, ":members", workspace.members.len() as i64)?;
    require_string(report_fields, ":lock", &expected_paths[0])?;
    require_string(report_fields, ":lock-h", &hashes[0])?;
    Ok(AuthorizedWorkspaceNew {
        lock_body: bodies.remove(0),
        workspace_body: bodies.remove(0),
        report,
    })
}

fn validate_documents(
    lock: &GenesisLock,
    workspace: &WorkspaceConfig,
    requested_workspace: &str,
    requested_policy: &str,
    requested_registry: Option<&str>,
    active_backend: &str,
) -> Result<(), String> {
    if workspace.workspace != requested_workspace
        || workspace.defaults.policy.as_deref() != Some(requested_policy)
        || workspace.defaults.registry.as_deref() != requested_registry
        || workspace.defaults.runtime_backend.as_deref() != Some(active_backend)
        || !workspace.tasks.is_empty()
    {
        return Err("workspace-new workspace contradicts request".to_string());
    }
    let expected_profiles = BTreeSet::from(["ci", "dev", "release"]);
    if workspace
        .profiles
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != expected_profiles
    {
        return Err("workspace-new profile inventory is not closed".to_string());
    }
    for (name, caps, backend) in [
        ("ci", "caps.ci.toml", "headless"),
        ("dev", "caps.toml", active_backend),
        ("release", "caps.release.toml", active_backend),
    ] {
        let profile = &workspace.profiles[name];
        if profile.caps_policy.as_deref() != Some(caps)
            || profile.registry.as_deref() != requested_registry
            || profile.policy.as_deref() != Some(requested_policy)
            || profile.runtime_backend.as_deref() != Some(backend)
            || profile.toolchain.is_some()
        {
            return Err(format!("workspace-new profile {name} contradicts request"));
        }
    }
    if workspace.members.is_empty()
        || workspace.members.iter().any(|member| {
            member.name.is_empty()
                || member.path.is_empty()
                || !matches!(member.role.as_deref(), Some("root" | "package"))
        })
    {
        return Err("workspace-new member inventory is invalid".to_string());
    }
    if lock.version != 2
        || lock.workspace != requested_workspace
        || lock.policy != requested_policy
        || !lock.requirements.is_empty()
        || !lock.locked.is_empty()
        || !lock.artifacts.is_empty()
    {
        return Err("workspace-new lock contradicts request".to_string());
    }
    match requested_registry {
        Some(registry)
            if lock.registries.len() == 1
                && lock.registries.get("default").map(String::as_str) == Some(registry) => {}
        None if lock.registries.is_empty() => {}
        _ => return Err("workspace-new lock registry contradicts request".to_string()),
    }
    Ok(())
}

fn preflight_paths(lock: &Path, workspace: &Path) -> Result<(), String> {
    if lock == workspace {
        return Err("workspace-new lock and workspace paths must differ".to_string());
    }
    for path in [lock, workspace] {
        if let Some(parent) = path.parent() {
            crate::pkg_scaffold::preflight_directory_chain(parent)?;
        }
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "refusing workspace-new destination symlink: {}",
                    path.display()
                ));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(format!(
                    "workspace-new destination is not a regular file: {}",
                    path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "inspect workspace-new destination {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
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
        .ok_or_else(|| format!("workspace-new result missing {name}"))
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, String> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        value => Err(format!(
            "workspace-new result {name} must be string, got {}",
            gc_coreform::print_term(value)
        )),
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
        Err(format!("workspace-new result {name} mismatch"))
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Int(value) if value.to_string() == expected.to_string() => Ok(()),
        _ => Err(format!("workspace-new result {name} mismatch")),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), String> {
    match field(fields, name)? {
        Term::Nil => Ok(()),
        _ => Err(format!("workspace-new result {name} must be nil")),
    }
}

fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
