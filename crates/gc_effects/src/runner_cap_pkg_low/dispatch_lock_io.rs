use super::*;
use crate::pkg_lock_read_authority::{
    PkgLockOpsDecision, PkgLockReadAuthority, PkgLockReadDecision,
};
#[cfg(any(test, feature = "parity-oracle"))]
#[path = "dispatch_lock_io/parity.rs"]
mod parity;
#[path = "dispatch_lock_io/save_lock.rs"]
mod save_lock;

#[expect(
    clippy::too_many_arguments,
    reason = "capability dispatch signatures are explicit by design"
)]
pub(super) fn dispatch_lock_io(
    op_eff: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    pkg_lock_read_authority: Option<&mut PkgLockReadAuthority>,
    pkg_lock_write_authority: Option<&mut PkgLockWriteAuthority>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    let _ = (policy, store, refs, budget, timeout_ms);
    match op_eff {
        "core/pkg-low::init" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(value) => value,
                Err(message) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        message,
                        Some(op),
                    ));
                }
            };
            let workspace = match payload_pkg_workspace(payload) {
                Ok(value) => value,
                Err(message) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        message,
                        Some(op),
                    ));
                }
            };
            let mut lock = gc_pkg::GenesisLock::empty(workspace);
            lock.policy =
                payload_pkg_policy(payload).unwrap_or_else(|| "policy:default-v0.1".to_string());
            if let Some(registry) = payload_pkg_registry_default(payload) {
                lock.registries.insert("default".to_string(), registry);
            }
            let bytes = lock.to_toml_canonical().into_bytes();
            let lock_hash = blake3::hash(&bytes).to_hex().to_string();
            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let lock_path = match sandbox_path_write(&base_dir, &lock_s, create_dirs) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            if let Err(error) = atomic_write_text(&lock_path, &bytes) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    error.to_string(),
                    Some(op),
                ));
            }
            Ok(lock_write_result(lock_s, lock_hash))
        }

        "core/pkg-low::add" => {
            let Some(authority) = pkg_lock_read_authority else {
                #[cfg(any(test, feature = "parity-oracle"))]
                {
                    return parity::dispatch_lock_ops_parity(op_eff, payload, pol, error_tok, op);
                }
                #[cfg(not(any(test, feature = "parity-oracle")))]
                {
                    return Err(lock_ops_authority_unavailable(op_eff));
                }
            };
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let bytes = match read_bounded_lock(&lock_path) {
                Ok(bytes) => bytes,
                Err(message) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-lock", message, Some(op)));
                }
            };
            let lock_write_path = match sandbox_path_write(&base_dir, &lock_s, false) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            match authority.add_lock_toml(&bytes, payload)? {
                PkgLockOpsDecision::Write { bytes, lock_hash } => {
                    if let Err(error) = atomic_write_text(&lock_write_path, &bytes) {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/io-error",
                            error.to_string(),
                            Some(op),
                        ));
                    }
                    Ok(lock_write_result(lock_s, lock_hash))
                }
                PkgLockOpsDecision::Error { code, message } => {
                    Ok(mk_error(error_tok, &code, message, Some(op)))
                }
                PkgLockOpsDecision::List { .. } => Err(EffectsError::Log(
                    "selfhost package lock ops authority returned list for add".to_string(),
                )),
            }
        }

        "core/pkg-low::list" => {
            let Some(authority) = pkg_lock_read_authority else {
                #[cfg(any(test, feature = "parity-oracle"))]
                {
                    return parity::dispatch_lock_ops_parity(op_eff, payload, pol, error_tok, op);
                }
                #[cfg(not(any(test, feature = "parity-oracle")))]
                {
                    return Err(lock_ops_authority_unavailable(op_eff));
                }
            };
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let bytes = match read_bounded_lock(&lock_path) {
                Ok(bytes) => bytes,
                Err(message) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-lock", message, Some(op)));
                }
            };
            match authority.list_lock_toml(&bytes, payload)? {
                PkgLockOpsDecision::List {
                    locked,
                    requirements,
                } => {
                    let mut result = BTreeMap::new();
                    result.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
                    result.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
                    result.insert(TermOrdKey(Term::symbol(":requirements")), requirements);
                    result.insert(TermOrdKey(Term::symbol(":locked")), locked);
                    Ok(Value::data(Term::Map(result)))
                }
                PkgLockOpsDecision::Error { code, message } => {
                    Ok(mk_error(error_tok, &code, message, Some(op)))
                }
                PkgLockOpsDecision::Write { .. } => Err(EffectsError::Log(
                    "selfhost package lock ops authority returned write for list".to_string(),
                )),
            }
        }

        "core/pkg-low::load-lock" => {
            let Some(authority) = pkg_lock_read_authority else {
                #[cfg(any(test, feature = "parity-oracle"))]
                {
                    return dispatch_load_lock_parity(payload, pol, error_tok, op);
                }
                #[cfg(not(any(test, feature = "parity-oracle")))]
                {
                    return Err(EffectsError::Log(
                        "core/pkg-low::load-lock requires the artifact-loaded GenesisCode lock read authority"
                            .to_string(),
                    ));
                }
            };
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let bytes = match read_bounded_lock(&lock_path) {
                Ok(bytes) => bytes,
                Err(message) => {
                    return Ok(mk_error(error_tok, "core/pkg/bad-lock", message, Some(op)));
                }
            };
            match authority.read_toml(&bytes)? {
                PkgLockReadDecision::Lock(Term::Map(mut lock)) => {
                    lock.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
                    lock.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
                    Ok(Value::data(Term::Map(lock)))
                }
                PkgLockReadDecision::Lock(_) => Err(EffectsError::Log(
                    "selfhost package lock read authority returned a non-map lock".to_string(),
                )),
                PkgLockReadDecision::Error { code, message } => {
                    Ok(mk_error(error_tok, &code, message, Some(op)))
                }
            }
        }
        "core/pkg-low::load-package" => handle_load_package(payload, pol, error_tok, op),
        "core/pkg-low::save-lock" => {
            save_lock::dispatch_save_lock(payload, pol, pkg_lock_write_authority, error_tok, op)
        }

        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op-eff",
            format!("core/pkg-low dispatch received unsupported op_eff: {op_eff}"),
            Some(op),
        )),
    }
}

fn lock_write_result(lock_path: String, lock_hash: String) -> Value {
    Value::data(Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (TermOrdKey(Term::symbol(":lock")), Term::Str(lock_path)),
            (TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_hash)),
        ]
        .into_iter()
        .collect(),
    ))
}

#[cfg(not(any(test, feature = "parity-oracle")))]
fn lock_ops_authority_unavailable(operation: &str) -> EffectsError {
    EffectsError::Log(format!(
        "{operation} requires the artifact-loaded GenesisCode lock ops authority"
    ))
}

#[cfg(any(test, feature = "parity-oracle"))]
fn dispatch_load_lock_parity(
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
    op: &str,
) -> Result<Value, EffectsError> {
    let lock_s = match payload_pkg_lock(payload) {
        Ok(path) => path,
        Err(error) => {
            return Ok(mk_error(error_tok, "core/pkg/bad-payload", error, Some(op)));
        }
    };
    let base_dir = effective_base_dir(pol)?;
    let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
        Ok(path) => path,
        Err(error) => {
            return Ok(mk_error(
                error_tok,
                "core/pkg/missing-lock",
                error.to_string(),
                Some(op),
            ));
        }
    };
    let lock = match gc_pkg::GenesisLock::load(&lock_path) {
        Ok(lock) => lock,
        Err(error) => {
            return Ok(mk_error(
                error_tok,
                "core/pkg/bad-lock",
                error.to_string(),
                Some(op),
            ));
        }
    };
    Ok(Value::data(legacy_lock_term(lock_s, lock)))
}

#[cfg(any(test, feature = "parity-oracle"))]
fn legacy_lock_term(lock_path: String, lock: gc_pkg::GenesisLock) -> Term {
    let requirements = lock
        .requirements
        .into_iter()
        .map(|(name, requirement)| {
            let update_policy = match requirement.update_policy {
                gc_pkg::UpdatePolicy::Manual => ":manual",
                gc_pkg::UpdatePolicy::Auto => ":auto",
            };
            (
                TermOrdKey(Term::Str(name)),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":registry")),
                            requirement.registry.map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selector")),
                            Term::Str(requirement.selector),
                        ),
                        (
                            TermOrdKey(Term::symbol(":update-policy")),
                            Term::symbol(update_policy),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            )
        })
        .collect();
    let locked = lock
        .locked
        .into_iter()
        .map(|(name, entry)| {
            (
                TermOrdKey(Term::Str(name)),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":commit")),
                            entry.commit.map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":exports_hash")),
                            entry.exports_hash.map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":registry")),
                            entry.registry.map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":resolved-ref")),
                            entry.resolved_ref.map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":snapshot")),
                            Term::Str(entry.snapshot),
                        ),
                        (
                            TermOrdKey(Term::symbol(":source_selector")),
                            if entry.source_selector.is_empty() {
                                Term::Nil
                            } else {
                                Term::Str(entry.source_selector)
                            },
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            )
        })
        .collect();
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":artifacts")),
                Term::Map(
                    lock.artifacts
                        .into_iter()
                        .map(|(key, value)| (TermOrdKey(Term::Str(key)), Term::Str(value)))
                        .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":lock")), Term::Str(lock_path)),
            (TermOrdKey(Term::symbol(":locked")), Term::Map(locked)),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (TermOrdKey(Term::symbol(":policy")), Term::Str(lock.policy)),
            (
                TermOrdKey(Term::symbol(":registries")),
                Term::Map(
                    lock.registries
                        .into_iter()
                        .map(|(key, value)| (TermOrdKey(Term::Str(key)), Term::Str(value)))
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":requirements")),
                Term::Map(requirements),
            ),
            (
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(lock.workspace),
            ),
        ]
        .into_iter()
        .collect(),
    )
}
