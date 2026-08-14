use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, Value};
use gc_pkg::{GenesisLock, LockedEntry, Requirement};

use super::LocalPkgResult;

const AUTHORITY_BINDING: &str = "core/pkg::workspace-remove-authority";
const LOCK_WRITE_BINDING: &str = "core/pkg::lock-write-authority";
const REQUEST_KIND: &str = "genesis/pkg-workspace-remove-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-workspace-remove-authority-result-v0.1";
const LOCK_WRITE_REQUEST_KIND: &str = "genesis/pkg-lock-write-authority-request-v0.1";
const LOCK_WRITE_RESULT_KIND: &str = "genesis/pkg-lock-write-authority-result-v0.1";

struct AuthorizedRemovePlan {
    model: Term,
    removed: bool,
}

pub(super) fn handle_remove(
    cli: &crate::Cli,
    name: &str,
    lock_path: &Path,
) -> Result<LocalPkgResult, String> {
    let original = GenesisLock::load(lock_path).map_err(|error| error.to_string())?;
    let request = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(REQUEST_KIND.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock_path.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":model")),
                lock_model_term(&original)?,
            ),
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str(name.to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    );
    let request_hash = hex32(hash_term(&request));
    let mut context = crate::mk_ctx(cli);
    let prelude = crate::build_prelude(&mut context);
    let mut environment = prelude.env;
    crate::load_selfhost_toolchain(cli, &mut context, &mut environment)
        .map_err(|error| format!("load workspace-remove authority: {error:?}"))?;
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
    let plan = decode_plan(value, &request_hash, name, lock_path)?;
    let writer_request = lock_write_request(lock_path, plan.model)?;
    let writer_hash = hex32(hash_term(&writer_request));
    let writer = environment
        .get(LOCK_WRITE_BINDING)
        .ok_or_else(|| format!("missing binding {LOCK_WRITE_BINDING}"))?;
    let writer_value = writer
        .apply(&mut context, Value::data(writer_request))
        .map_err(|error| format!("{LOCK_WRITE_BINDING} failed: {error}"))?;
    let (bytes, lock_hash) = decode_lock_write(writer_value, &writer_hash)?;
    let body = std::str::from_utf8(&bytes)
        .map_err(|_| "workspace-remove lock bytes must be UTF-8".to_string())?;
    let candidate = GenesisLock::from_toml_str(lock_path, body)
        .map_err(|error| format!("workspace-remove lock document is invalid: {error}"))?;
    validate_candidate(&original, &candidate, name)?;
    let expected_removed =
        original.requirements.contains_key(name) || original.locked.contains_key(name);
    if plan.removed != expected_removed {
        return Err("workspace-remove authority :removed contradicts input".to_string());
    }
    let report = map([
        (":lock", Term::Str(lock_path.display().to_string())),
        (":lock-h", Term::Str(lock_hash)),
        (":name", Term::Str(name.to_string())),
        (":ok", Term::Bool(true)),
        (":removed", Term::Bool(plan.removed)),
    ]);

    preflight_lock_path(lock_path)?;
    crate::pkg_scaffold::atomic_write_text(lock_path, &bytes)
        .map_err(|error| format!("write {}: {error}", lock_path.display()))?;

    Ok(LocalPkgResult {
        kind: "genesis/pkg-remove-v0.1",
        log_op: "pkg-remove",
        program_hash: hash_term(&report),
        value: report,
    })
}

fn decode_plan(
    value: Value,
    request_hash: &str,
    requested_name: &str,
    lock_path: &Path,
) -> Result<AuthorizedRemovePlan, String> {
    let Some(Term::Map(envelope)) = value.to_plain_term() else {
        return Err("workspace-remove authority returned non-map".to_string());
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
        "workspace-remove envelope",
    )?;
    require_string(&envelope, ":kind", RESULT_KIND)?;
    require_int(&envelope, ":v", 1)?;
    require_string(&envelope, ":request-h", request_hash)?;
    match field(&envelope, ":ok")? {
        Term::Bool(false) => {
            require_nil(&envelope, ":value")?;
            require_string(&envelope, ":code", "core/pkg/bad-workspace-remove")?;
            return Err(format!(
                "core/pkg/bad-workspace-remove: {}",
                required_string(&envelope, ":message")?
            ));
        }
        Term::Bool(true) => {
            require_nil(&envelope, ":code")?;
            require_nil(&envelope, ":message")?;
        }
        _ => return Err("workspace-remove envelope :ok must be bool".to_string()),
    }

    let Term::Map(result) = field(&envelope, ":value")? else {
        return Err("workspace-remove result :value must be map".to_string());
    };
    require_exact_fields(
        result,
        &[":lock", ":model", ":name", ":removed"],
        "workspace-remove result",
    )?;
    require_string(result, ":lock", &lock_path.display().to_string())?;
    require_string(result, ":name", requested_name)?;
    let removed = match field(result, ":removed")? {
        Term::Bool(value) => *value,
        _ => return Err("workspace-remove result :removed must be bool".to_string()),
    };
    let model = field(result, ":model")?.clone();
    require_exact_term_map(
        &model,
        &[
            ":artifacts",
            ":locked",
            ":policy",
            ":registries",
            ":requirements",
            ":version",
            ":workspace",
        ],
        "workspace-remove model",
    )?;
    Ok(AuthorizedRemovePlan { model, removed })
}

fn lock_write_request(lock_path: &Path, model: Term) -> Result<Term, String> {
    let Term::Map(mut payload) = model else {
        return Err("workspace-remove model must be map".to_string());
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
            "lock-write authority rejected remove model: {}",
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

fn require_exact_term_map(term: &Term, names: &[&str], label: &str) -> Result<(), String> {
    let Term::Map(fields) = term else {
        return Err(format!("{label} must be map"));
    };
    require_exact_fields(fields, names, label)
}

fn validate_candidate(
    original: &GenesisLock,
    candidate: &GenesisLock,
    removed_name: &str,
) -> Result<(), String> {
    for (matches, label) in [
        (candidate.version == original.version, "version"),
        (candidate.workspace == original.workspace, "workspace"),
        (candidate.policy == original.policy, "policy"),
        (candidate.registries == original.registries, "registries"),
        (candidate.artifacts == original.artifacts, "artifacts"),
        (
            !candidate.requirements.contains_key(removed_name),
            "target requirement",
        ),
        (
            !candidate.locked.contains_key(removed_name),
            "target locked entry",
        ),
    ] {
        if !matches {
            return Err(format!(
                "workspace-remove lock contradicts requested mutation: {label}"
            ));
        }
    }
    validate_requirements_except(
        &original.requirements,
        &candidate.requirements,
        removed_name,
    )?;
    validate_locked_except(&original.locked, &candidate.locked, removed_name)?;
    Ok(())
}

fn validate_requirements_except(
    original: &BTreeMap<String, Requirement>,
    candidate: &BTreeMap<String, Requirement>,
    removed_name: &str,
) -> Result<(), String> {
    let expected_len = original.len() - usize::from(original.contains_key(removed_name));
    if candidate.len() != expected_len {
        return Err(format!(
            "workspace-remove lock contradicts requested mutation: requirement inventory expected {expected_len}, got {}",
            candidate.len()
        ));
    }
    for (name, value) in original
        .iter()
        .filter(|(name, _)| name.as_str() != removed_name)
    {
        let other = candidate.get(name).ok_or_else(|| {
            "workspace-remove lock contradicts requested mutation: missing requirement".to_string()
        })?;
        for (matches, label) in [
            (value.selector == other.selector, "selector"),
            (value.update_policy == other.update_policy, "update policy"),
            (value.registry == other.registry, "requirement registry"),
            (value.strategy == other.strategy, "strategy"),
            (value.tag_policy == other.tag_policy, "tag policy"),
        ] {
            if !matches {
                return Err(format!(
                    "workspace-remove lock contradicts requested mutation: requirement {label}"
                ));
            }
        }
    }
    Ok(())
}

fn validate_locked_except(
    original: &BTreeMap<String, LockedEntry>,
    candidate: &BTreeMap<String, LockedEntry>,
    removed_name: &str,
) -> Result<(), String> {
    if candidate.len() != original.len() - usize::from(original.contains_key(removed_name)) {
        return Err(
            "workspace-remove lock contradicts requested mutation: locked inventory".to_string(),
        );
    }
    for (name, value) in original
        .iter()
        .filter(|(name, _)| name.as_str() != removed_name)
    {
        let other = candidate.get(name).ok_or_else(|| {
            "workspace-remove lock contradicts requested mutation: missing locked entry".to_string()
        })?;
        for (matches, label) in [
            (value.commit == other.commit, "commit"),
            (value.snapshot == other.snapshot, "snapshot"),
            (value.registry == other.registry, "locked registry"),
            (
                value.source_selector == other.source_selector,
                "source selector",
            ),
            (value.resolved_ref == other.resolved_ref, "resolved ref"),
            (value.exports_hash == other.exports_hash, "exports hash"),
            (
                value.environment_fingerprint == other.environment_fingerprint,
                "environment fingerprint",
            ),
        ] {
            if !matches {
                return Err(format!(
                    "workspace-remove lock contradicts requested mutation: locked {label}"
                ));
            }
        }
    }
    Ok(())
}

fn lock_model_term(lock: &GenesisLock) -> Result<Term, String> {
    let version = i64::try_from(lock.version)
        .map_err(|_| "lock version exceeds protocol integer range".to_string())?;
    let string_map = |values: &BTreeMap<String, String>| {
        Term::Map(
            values
                .iter()
                .map(|(name, value)| {
                    (
                        TermOrdKey(Term::Str(name.clone())),
                        Term::Str(value.clone()),
                    )
                })
                .collect(),
        )
    };
    let requirements = lock
        .requirements
        .iter()
        .map(|(name, value)| {
            let update_policy = match value.update_policy {
                gc_pkg::UpdatePolicy::Manual => ":manual",
                gc_pkg::UpdatePolicy::Auto => ":auto",
            };
            (
                TermOrdKey(Term::Str(name.clone())),
                map([
                    (
                        ":registry",
                        value.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                    ),
                    (":selector", Term::Str(value.selector.clone())),
                    (
                        ":strategy",
                        Term::symbol(format!(":{}", value.strategy.as_str())),
                    ),
                    (
                        ":tag-policy",
                        value.tag_policy.clone().map(Term::Str).unwrap_or(Term::Nil),
                    ),
                    (":update-policy", Term::symbol(update_policy)),
                ]),
            )
        })
        .collect();
    let locked = lock
        .locked
        .iter()
        .map(|(name, value)| {
            (
                TermOrdKey(Term::Str(name.clone())),
                map([
                    (
                        ":commit",
                        value.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                    ),
                    (
                        ":environment-fingerprint",
                        value
                            .environment_fingerprint
                            .clone()
                            .map(Term::Str)
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        ":exports_hash",
                        value
                            .exports_hash
                            .clone()
                            .map(Term::Str)
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        ":registry",
                        value.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                    ),
                    (
                        ":resolved-ref",
                        value
                            .resolved_ref
                            .clone()
                            .map(Term::Str)
                            .unwrap_or(Term::Nil),
                    ),
                    (":snapshot", Term::Str(value.snapshot.clone())),
                    (":source_selector", Term::Str(value.source_selector.clone())),
                ]),
            )
        })
        .collect();
    Ok(map([
        (":artifacts", string_map(&lock.artifacts)),
        (":locked", Term::Map(locked)),
        (":policy", Term::Str(lock.policy.clone())),
        (":registries", string_map(&lock.registries)),
        (":requirements", Term::Map(requirements)),
        (":version", Term::Int(version.into())),
        (":workspace", Term::Str(lock.workspace.clone())),
    ]))
}

fn preflight_lock_path(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        crate::pkg_scaffold::preflight_directory_chain(parent)?;
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("inspect workspace-remove lock {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "refusing workspace-remove lock symlink: {}",
            path.display()
        ));
    }
    if !metadata.is_file() {
        return Err(format!(
            "workspace-remove lock is not a regular file: {}",
            path.display()
        ));
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
        .ok_or_else(|| format!("workspace-remove result missing {name}"))
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, String> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        value => Err(format!(
            "workspace-remove result {name} must be string, got {}",
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
        Err(format!("workspace-remove result {name} mismatch"))
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Int(value) if value.to_string() == expected.to_string() => Ok(()),
        _ => Err(format!("workspace-remove result {name} mismatch")),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), String> {
    match field(fields, name)? {
        Term::Nil => Ok(()),
        _ => Err(format!("workspace-remove result {name} must be nil")),
    }
}

fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
