use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, Value};
use gc_pkg::{GenesisLock, PackageManifest, ResolutionStrategy, UpdatePolicy, WorkspaceConfig};

use super::LocalPkgResult;

const AUTHORITY_BINDING: &str = "core/pkg::workspace-migrate-authority";
const LOCK_WRITE_BINDING: &str = "core/pkg::lock-write-authority";
const REQUEST_KIND: &str = "genesis/pkg-workspace-migrate-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-workspace-migrate-authority-result-v0.1";
const LOCK_WRITE_REQUEST_KIND: &str = "genesis/pkg-lock-write-authority-request-v0.1";
const LOCK_WRITE_RESULT_KIND: &str = "genesis/pkg-lock-write-authority-result-v0.1";

struct AuthorizedMigration {
    lock_model: Term,
    report: BTreeMap<TermOrdKey, Term>,
    workspace_body: String,
}

pub(super) fn handle_migrate(
    cli: &crate::Cli,
    package_path: &Path,
    lock_path: &Path,
    workspace_path: &Path,
    workspace_override: Option<&str>,
    registry_default: Option<&str>,
) -> Result<LocalPkgResult, String> {
    let (manifest, package_dir) =
        PackageManifest::load(package_path).map_err(|error| error.to_string())?;
    let member_path = relative_to_cwd_or_literal(&package_dir);
    let request = migration_request(
        &manifest,
        package_path,
        &member_path,
        lock_path,
        workspace_path,
        workspace_override,
        registry_default,
    );
    let request_hash = hex32(hash_term(&request));
    let mut context = crate::mk_ctx(cli);
    let prelude = crate::build_prelude(&mut context);
    let mut environment = prelude.env;
    crate::load_selfhost_toolchain(cli, &mut context, &mut environment)
        .map_err(|error| format!("load workspace-migrate authority: {error:?}"))?;
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
    let mut authorized = decode_authorized(
        value,
        &request_hash,
        &manifest,
        package_path,
        &member_path,
        lock_path,
        workspace_path,
        workspace_override,
        registry_default,
    )?;

    let writer_request = lock_write_request(lock_path, authorized.lock_model.clone())?;
    let writer_hash = hex32(hash_term(&writer_request));
    let writer = environment
        .get(LOCK_WRITE_BINDING)
        .ok_or_else(|| format!("missing binding {LOCK_WRITE_BINDING}"))?;
    let writer_value = writer
        .apply(&mut context, Value::data(writer_request))
        .map_err(|error| format!("{LOCK_WRITE_BINDING} failed: {error}"))?;
    let (lock_bytes, lock_hash) = decode_lock_write(writer_value, &writer_hash)?;
    let lock_body = std::str::from_utf8(&lock_bytes)
        .map_err(|_| "workspace-migrate lock bytes must be UTF-8".to_string())?;

    validate_documents(
        &GenesisLock::from_toml_str(lock_path, lock_body)
            .map_err(|error| format!("workspace-migrate lock document is invalid: {error}"))?,
        &WorkspaceConfig::from_toml_str(workspace_path, &authorized.workspace_body)
            .map_err(|error| format!("workspace-migrate workspace document is invalid: {error}"))?,
        &manifest,
        package_path,
        &member_path,
        workspace_override,
        registry_default,
    )?;
    authorized
        .report
        .insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_hash));
    let report = Term::Map(authorized.report);

    preflight_paths(lock_path, workspace_path)?;
    create_parent(lock_path)?;
    create_parent(workspace_path)?;
    crate::pkg_scaffold::atomic_write_text(lock_path, &lock_bytes)
        .map_err(|error| format!("write {}: {error}", lock_path.display()))?;
    crate::pkg_scaffold::atomic_write_text(workspace_path, authorized.workspace_body.as_bytes())
        .map_err(|error| format!("write {}: {error}", workspace_path.display()))?;

    Ok(LocalPkgResult {
        kind: "genesis/pkg-migrate-v0.1",
        log_op: "pkg-migrate",
        program_hash: hash_term(&report),
        value: report,
    })
}

#[allow(clippy::too_many_arguments)]
fn migration_request(
    manifest: &PackageManifest,
    package_path: &Path,
    member_path: &str,
    lock_path: &Path,
    workspace_path: &Path,
    workspace_override: Option<&str>,
    registry_default: Option<&str>,
) -> Term {
    let dependencies = manifest
        .dependencies
        .iter()
        .map(|dependency| {
            map([
                (
                    ":hash",
                    dependency.hash.clone().map(Term::Str).unwrap_or(Term::Nil),
                ),
                (":name", Term::Str(dependency.name.clone())),
                (":path", Term::Str(dependency.path.clone())),
            ])
        })
        .collect();
    map([
        (":dependencies", Term::Vector(dependencies)),
        (":kind", Term::Str(REQUEST_KIND.to_string())),
        (":lock", Term::Str(lock_path.display().to_string())),
        (":member-path", Term::Str(member_path.to_string())),
        (":package-name", Term::Str(manifest.name.clone())),
        (
            ":package-path",
            Term::Str(package_path.display().to_string()),
        ),
        (
            ":registry-default",
            registry_default
                .map(|value| Term::Str(value.to_string()))
                .unwrap_or(Term::Nil),
        ),
        (":v", Term::Int(1.into())),
        (
            ":workspace",
            workspace_override
                .map(|value| Term::Str(value.to_string()))
                .unwrap_or(Term::Nil),
        ),
        (
            ":workspace-file",
            Term::Str(workspace_path.display().to_string()),
        ),
    ])
}

#[allow(clippy::too_many_arguments)]
fn decode_authorized(
    value: Value,
    request_hash: &str,
    manifest: &PackageManifest,
    package_path: &Path,
    member_path: &str,
    lock_path: &Path,
    workspace_path: &Path,
    workspace_override: Option<&str>,
    registry_default: Option<&str>,
) -> Result<AuthorizedMigration, String> {
    let Some(Term::Map(envelope)) = value.to_plain_term() else {
        return Err("workspace-migrate authority returned non-map".to_string());
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
        "workspace-migrate envelope",
    )?;
    require_string(&envelope, ":kind", RESULT_KIND)?;
    require_int(&envelope, ":v", 1)?;
    require_string(&envelope, ":request-h", request_hash)?;
    match field(&envelope, ":ok")? {
        Term::Bool(false) => {
            require_nil(&envelope, ":value")?;
            require_string(&envelope, ":code", "core/pkg/bad-workspace-migrate")?;
            return Err(format!(
                "core/pkg/bad-workspace-migrate: {}",
                required_string(&envelope, ":message")?
            ));
        }
        Term::Bool(true) => {
            require_nil(&envelope, ":code")?;
            require_nil(&envelope, ":message")?;
        }
        _ => return Err("workspace-migrate envelope :ok must be bool".to_string()),
    }
    let Term::Map(result) = field(&envelope, ":value")? else {
        return Err("workspace-migrate result :value must be map".to_string());
    };
    require_exact_fields(
        result,
        &[":lock-model", ":report", ":workspace-body"],
        "workspace-migrate result",
    )?;
    let lock_model = field(result, ":lock-model")?.clone();
    require_exact_term_map(
        &lock_model,
        &[
            ":artifacts",
            ":locked",
            ":policy",
            ":registries",
            ":requirements",
            ":version",
            ":workspace",
        ],
        "workspace-migrate lock model",
    )?;
    let workspace_body = required_string(result, ":workspace-body")?.to_string();
    let Term::Map(report) = field(result, ":report")? else {
        return Err("workspace-migrate report must be map".to_string());
    };
    require_exact_fields(
        report,
        &[
            ":dep-count",
            ":lock",
            ":lock-h",
            ":ok",
            ":workspace",
            ":workspace-file",
            ":workspace-h",
        ],
        "workspace-migrate report",
    )?;
    if field(report, ":ok")? != &Term::Bool(true) {
        return Err("workspace-migrate report :ok must be true".to_string());
    }
    let expected_workspace = workspace_override.unwrap_or(&manifest.name);
    require_string(report, ":workspace", expected_workspace)?;
    require_string(
        report,
        ":workspace-file",
        &workspace_path.display().to_string(),
    )?;
    require_string(report, ":lock", &lock_path.display().to_string())?;
    require_nil(report, ":lock-h")?;
    require_int(report, ":dep-count", manifest.dependencies.len() as i64)?;
    require_string(
        report,
        ":workspace-h",
        blake3::hash(workspace_body.as_bytes()).to_hex().as_str(),
    )?;

    let lock = lock_from_model(&lock_model)?;
    let workspace = WorkspaceConfig::from_toml_str(workspace_path, &workspace_body)
        .map_err(|error| format!("workspace-migrate workspace document is invalid: {error}"))?;
    validate_documents(
        &lock,
        &workspace,
        manifest,
        package_path,
        member_path,
        workspace_override,
        registry_default,
    )?;
    Ok(AuthorizedMigration {
        lock_model,
        report: report.clone(),
        workspace_body,
    })
}

fn lock_from_model(model: &Term) -> Result<GenesisLock, String> {
    let Term::Map(fields) = model else {
        return Err("workspace-migrate lock model must be map".to_string());
    };
    let workspace = required_string(fields, ":workspace")?.to_string();
    let policy = required_string(fields, ":policy")?.to_string();
    require_int(fields, ":version", 2)?;
    let registries = string_map(field(fields, ":registries")?, "registries")?;
    require_empty_map(field(fields, ":locked")?, "locked")?;
    require_empty_map(field(fields, ":artifacts")?, "artifacts")?;
    let requirements = requirement_map(field(fields, ":requirements")?)?;
    Ok(GenesisLock {
        version: 2,
        workspace,
        policy,
        registries,
        requirements,
        locked: BTreeMap::new(),
        artifacts: BTreeMap::new(),
    })
}

fn requirement_map(term: &Term) -> Result<BTreeMap<String, gc_pkg::Requirement>, String> {
    let Term::Map(entries) = term else {
        return Err("workspace-migrate requirements must be map".to_string());
    };
    entries
        .iter()
        .map(|(key, value)| {
            let Term::Str(name) = &key.0 else {
                return Err("workspace-migrate requirement name must be string".to_string());
            };
            let Term::Map(fields) = value else {
                return Err("workspace-migrate requirement must be map".to_string());
            };
            require_exact_fields(
                fields,
                &[
                    ":registry",
                    ":selector",
                    ":strategy",
                    ":tag-policy",
                    ":update-policy",
                ],
                "workspace-migrate requirement",
            )?;
            require_string(fields, ":registry", "default")?;
            require_symbol(fields, ":strategy", ":pinned")?;
            require_symbol(fields, ":update-policy", ":manual")?;
            require_nil(fields, ":tag-policy")?;
            Ok((
                name.clone(),
                gc_pkg::Requirement {
                    selector: required_string(fields, ":selector")?.to_string(),
                    update_policy: UpdatePolicy::Manual,
                    registry: Some("default".to_string()),
                    strategy: ResolutionStrategy::Pinned,
                    tag_policy: None,
                },
            ))
        })
        .collect()
}

fn string_map(term: &Term, label: &str) -> Result<BTreeMap<String, String>, String> {
    let Term::Map(entries) = term else {
        return Err(format!("workspace-migrate {label} must be map"));
    };
    entries
        .iter()
        .map(|(key, value)| match (&key.0, value) {
            (Term::Str(name), Term::Str(value)) => Ok((name.clone(), value.clone())),
            _ => Err(format!(
                "workspace-migrate {label} must map strings to strings"
            )),
        })
        .collect()
}

fn require_empty_map(term: &Term, label: &str) -> Result<(), String> {
    match term {
        Term::Map(entries) if entries.is_empty() => Ok(()),
        _ => Err(format!("workspace-migrate {label} must be empty map")),
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_documents(
    lock: &GenesisLock,
    workspace: &WorkspaceConfig,
    manifest: &PackageManifest,
    package_path: &Path,
    member_path: &str,
    workspace_override: Option<&str>,
    registry_default: Option<&str>,
) -> Result<(), String> {
    let expected_workspace = workspace_override.unwrap_or(&manifest.name);
    if lock.version != 2
        || lock.workspace != expected_workspace
        || lock.policy != "policy:default-v0.1"
        || !lock.locked.is_empty()
        || !lock.artifacts.is_empty()
    {
        return Err("workspace-migrate lock contradicts request".to_string());
    }
    let expected_registries = registry_default
        .map(|value| BTreeMap::from([("default".to_string(), value.to_string())]))
        .unwrap_or_default();
    if lock.registries != expected_registries {
        return Err("workspace-migrate registry model contradicts request".to_string());
    }
    let mut expected_requirements = BTreeMap::new();
    for dependency in &manifest.dependencies {
        if let Some(hash) = dependency.hash.as_deref()
            && is_hash_hex_64(hash)
        {
            expected_requirements.insert(dependency.name.as_str(), hash);
        }
    }
    if lock.requirements.len() != expected_requirements.len() {
        return Err("workspace-migrate requirement inventory contradicts manifest".to_string());
    }
    for (name, hash) in expected_requirements {
        let requirement = lock.requirements.get(name).ok_or_else(|| {
            "workspace-migrate requirement inventory contradicts manifest".to_string()
        })?;
        if requirement.selector != format!("snapshot:{hash}")
            || requirement.update_policy != UpdatePolicy::Manual
            || requirement.registry.as_deref() != Some("default")
            || requirement.strategy != ResolutionStrategy::Pinned
            || requirement.tag_policy.is_some()
        {
            return Err("workspace-migrate requirement contradicts manifest".to_string());
        }
    }

    if workspace.version != 1
        || workspace.workspace != expected_workspace
        || workspace.defaults.registry.as_deref() != registry_default
        || workspace.defaults.policy.as_deref() != Some("policy:default-v0.1")
        || workspace.defaults.toolchain.is_some()
        || workspace.defaults.runtime_backend.is_some()
        || !workspace.profiles.is_empty()
        || workspace.members.len() != 1
        || workspace.tasks.len() != 2
    {
        return Err("workspace-migrate workspace contradicts request".to_string());
    }
    let member = &workspace.members[0];
    if member.name != manifest.name
        || member.path != member_path
        || member.role.as_deref() != Some("package")
    {
        return Err("workspace-migrate member contradicts manifest".to_string());
    }
    let expected_package_path = package_path.display().to_string();
    for name in ["pack", "test"] {
        let task = workspace
            .tasks
            .get(name)
            .ok_or_else(|| "workspace-migrate task inventory contradicts request".to_string())?;
        if task.cmd != name
            || task.pkg.as_deref() != Some(expected_package_path.as_str())
            || task.file.is_some()
            || !task.args.is_empty()
        {
            return Err(format!("workspace-migrate {name} task contradicts request"));
        }
    }
    Ok(())
}

fn lock_write_request(lock_path: &Path, model: Term) -> Result<Term, String> {
    let Term::Map(mut payload) = model else {
        return Err("workspace-migrate lock model must be map".to_string());
    };
    payload.insert(
        TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock_path.display().to_string()),
    );
    Ok(map([
        (":kind", Term::Str(LOCK_WRITE_REQUEST_KIND.to_string())),
        (":op", Term::symbol(":write")),
        (":payload", Term::Map(payload)),
        (":v", Term::Int(1.into())),
    ]))
}

fn decode_lock_write(value: Value, request_hash: &str) -> Result<(Vec<u8>, String), String> {
    let Some(Term::Map(fields)) = value.to_plain_term() else {
        return Err("lock-write authority returned non-map".to_string());
    };
    require_exact_fields(
        &fields,
        &[
            ":bytes",
            ":code",
            ":kind",
            ":lock-h",
            ":message",
            ":ok",
            ":request-h",
            ":v",
        ],
        "lock-write envelope",
    )?;
    require_string(&fields, ":kind", LOCK_WRITE_RESULT_KIND)?;
    require_string(&fields, ":request-h", request_hash)?;
    require_int(&fields, ":v", 1)?;
    if field(&fields, ":ok")? != &Term::Bool(true) {
        return Err(format!(
            "lock-write authority rejected migration model: {}",
            required_string(&fields, ":message")?
        ));
    }
    require_nil(&fields, ":code")?;
    require_nil(&fields, ":message")?;
    let Term::Bytes(bytes) = field(&fields, ":bytes")? else {
        return Err("lock-write :bytes must be bytes".to_string());
    };
    let lock_hash = required_string(&fields, ":lock-h")?.to_string();
    if blake3::hash(bytes).to_hex().as_str() != lock_hash {
        return Err("lock-write bytes/hash contradiction".to_string());
    }
    Ok((bytes.to_vec(), lock_hash))
}

fn relative_to_cwd_or_literal(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        return ".".to_string();
    }
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(relative) = path.strip_prefix(cwd)
    {
        let rendered = relative.display().to_string();
        if !rendered.is_empty() {
            return rendered;
        }
    }
    path.display().to_string()
}

fn is_hash_hex_64(value: &str) -> bool {
    value.len() == 64 && value.as_bytes().iter().all(|byte| byte.is_ascii_hexdigit())
}

fn preflight_paths(lock_path: &Path, workspace_path: &Path) -> Result<(), String> {
    if lock_path == workspace_path {
        return Err("workspace-migrate lock and workspace paths must differ".to_string());
    }
    for path in [lock_path, workspace_path] {
        if let Some(parent) = path.parent() {
            crate::pkg_scaffold::preflight_directory_chain(parent)?;
        }
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "refusing workspace-migrate destination symlink: {}",
                    path.display()
                ));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(format!(
                    "workspace-migrate destination is not a regular file: {}",
                    path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "inspect workspace-migrate destination {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn create_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create migration directory {}: {error}", parent.display()))?;
    }
    Ok(())
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn require_exact_term_map(term: &Term, names: &[&str], label: &str) -> Result<(), String> {
    let Term::Map(fields) = term else {
        return Err(format!("{label} must be map"));
    };
    require_exact_fields(fields, names, label)
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
        .ok_or_else(|| format!("workspace-migrate result missing {name}"))
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, String> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        value => Err(format!(
            "workspace-migrate result {name} must be string, got {}",
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
        Err(format!("workspace-migrate result {name} mismatch"))
    }
}

fn require_symbol(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Symbol(value) if value == expected => Ok(()),
        _ => Err(format!("workspace-migrate result {name} mismatch")),
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Int(value) if value.to_string() == expected.to_string() => Ok(()),
        _ => Err(format!("workspace-migrate result {name} mismatch")),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), String> {
    match field(fields, name)? {
        Term::Nil => Ok(()),
        _ => Err(format!("workspace-migrate result {name} must be nil")),
    }
}

fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
