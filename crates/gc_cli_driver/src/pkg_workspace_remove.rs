use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, Value};

use super::LocalPkgResult;

const AUTHORITY_BINDING: &str = "core/pkg::workspace-remove-authority";
const REQUEST_KIND: &str = "genesis/pkg-workspace-remove-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-workspace-remove-authority-result-v0.1";

struct AuthorizedRemovePlan {
    model: Term,
    removed: bool,
}

pub(super) fn handle_remove(
    cli: &crate::Cli,
    name: &str,
    lock_path: &Path,
) -> Result<LocalPkgResult, String> {
    preflight_lock_path(lock_path)?;
    let lock_bytes = crate::pkg_lock_model_authority::read_bounded(lock_path)?;
    let mut context = crate::mk_ctx(cli);
    let prelude = crate::build_prelude(&mut context);
    let mut environment = prelude.env;
    crate::load_selfhost_toolchain(cli, &mut context, &mut environment)
        .map_err(|error| format!("load workspace-remove authority: {error:?}"))?;
    let original =
        crate::pkg_lock_model_authority::authorize_bytes(&mut context, &environment, &lock_bytes)?;
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
            (TermOrdKey(Term::symbol(":model")), original.clone()),
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
    let (bytes, lock_hash) = crate::pkg_lock_model_authority::render_canonical(
        &mut context,
        &environment,
        lock_path,
        &plan.model,
    )?;
    let candidate =
        crate::pkg_lock_model_authority::authorize_bytes(&mut context, &environment, &bytes)?;
    if candidate != plan.model {
        return Err("workspace-remove written lock model contradicts authorized plan".to_string());
    }
    let expected_removed = model_contains_name(&original, ":requirements", name)?
        || model_contains_name(&original, ":locked", name)?;
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

fn require_exact_term_map(term: &Term, names: &[&str], label: &str) -> Result<(), String> {
    let Term::Map(fields) = term else {
        return Err(format!("{label} must be map"));
    };
    require_exact_fields(fields, names, label)
}

fn model_contains_name(model: &Term, collection: &str, name: &str) -> Result<bool, String> {
    let Term::Map(fields) = model else {
        return Err("workspace-remove model must be map".to_string());
    };
    let Term::Map(entries) = field(fields, collection)? else {
        return Err(format!("workspace-remove model {collection} must be map"));
    };
    Ok(entries.contains_key(&TermOrdKey(Term::Str(name.to_string()))))
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
