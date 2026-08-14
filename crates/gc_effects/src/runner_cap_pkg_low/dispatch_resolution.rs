use super::*;
use crate::pkg_lock_read_authority::PkgLockModelDecision;
use crate::pkg_lock_write_authority::{PkgLockWriteAuthority, PkgLockWriteDecision};

#[path = "dispatch_resolution/install_verify.rs"]
mod install_verify;
#[path = "dispatch_resolution/rationale_diagnostics.rs"]
mod rationale_diagnostics;
#[path = "dispatch_resolution/workflow.rs"]
mod workflow;

use rationale_diagnostics::annotate_requirement_resolution_error;
use workflow::{execute_workflow, finalize_workflow};

#[expect(
    clippy::too_many_arguments,
    reason = "capability dispatch signatures are explicit by design"
)]
pub(super) fn dispatch_resolution(
    op_eff: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    mut lock_authority: Option<&mut PkgLockReadAuthority>,
    lock_write_authority: Option<&mut PkgLockWriteAuthority>,
    mut identity_authority: Option<&mut PkgResolutionIdentityAuthority>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    match op_eff {
        "core/pkg-low::info" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let name = match payload_pkg_name(payload) {
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
            let l = match load_lock_model(lock_authority.as_deref_mut(), &lock_path, error_tok, op)
            {
                Ok(x) => x,
                Err(error) => return Ok(error),
            };

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
            m.insert(
                TermOrdKey(Term::symbol(":requirement")),
                l.requirements
                    .get(&name)
                    .map(|r| {
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":selector")),
                                    Term::Str(r.selector.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":update-policy")),
                                    Term::Symbol(match r.update_policy {
                                        gc_pkg::UpdatePolicy::Manual => ":manual".to_string(),
                                        gc_pkg::UpdatePolicy::Auto => ":auto".to_string(),
                                    }),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":registry")),
                                    r.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":strategy")),
                                    Term::Symbol(format!(":{}", r.strategy.as_str())),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":tag-policy")),
                                    r.tag_policy.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .unwrap_or(Term::Nil),
            );
            m.insert(
                TermOrdKey(Term::symbol(":locked")),
                l.locked
                    .get(&name)
                    .map(|le| {
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":commit")),
                                    le.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":snapshot")),
                                    Term::Str(le.snapshot.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":resolved-ref")),
                                    le.resolved_ref.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":environment-fingerprint")),
                                    le.environment_fingerprint
                                        .clone()
                                        .map(Term::Str)
                                        .unwrap_or(Term::Nil),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .unwrap_or(Term::Nil),
            );
            Ok(Value::data(Term::Map(m)))
        }

        "core/pkg-low::lock" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::lock".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg-low::lock".to_string())
            })?;
            let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);
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
            let mut l =
                match load_lock_model(lock_authority.as_deref_mut(), &lock_path, error_tok, op) {
                    Ok(x) => x,
                    Err(error) => return Ok(error),
                };

            let executed = match execute_workflow(
                identity_authority.as_deref_mut(),
                PkgResolutionWorkflow::Lock,
                &[],
                &lock_s,
                &l,
                store,
                refs,
                policy,
                pol,
                budget,
                timeout_ms,
                error_tok,
                op,
            ) {
                Ok(value) => value,
                Err(value) => return Ok(value),
            };
            let finalized = match finalize_workflow(
                identity_authority.as_deref_mut(),
                PkgResolutionWorkflow::Lock,
                &[],
                &lock_s,
                &mut l,
                executed,
                store,
                strict,
                error_tok,
                op,
            ) {
                Ok(value) => value,
                Err(value) => return Ok(value),
            };
            let lock_rationale_artifact = l
                .artifacts
                .get("lock_resolution_rationale")
                .cloned()
                .unwrap_or_default();
            let workspace_root = l
                .artifacts
                .get("root_workspace_snapshot")
                .cloned()
                .unwrap_or_default();

            let (bytes, lock_h) =
                match render_resolved_lock(lock_write_authority, &lock_s, &l, error_tok, op) {
                    Ok(result) => result,
                    Err(error) => return Ok(error),
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
            if let Err(e) = atomic_write_text(&lock_write_path, &bytes) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":lock-h")),
                Term::Str(lock_h.clone()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":locked-count")),
                Term::Int(finalized.locked_count.into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":rationale-count")),
                Term::Int((finalized.rationale.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":rationale")),
                Term::Vector(finalized.rationale),
            );
            m.insert(
                TermOrdKey(Term::symbol(":rationale-artifact")),
                Term::Str(lock_rationale_artifact.clone()),
            );
            m.insert(TermOrdKey(Term::symbol(":strict")), Term::Bool(strict));
            m.insert(
                TermOrdKey(Term::symbol(":workspace-root")),
                Term::Str(workspace_root.clone()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":workspace-root")),
                            Term::Str(workspace_root),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock-h")),
                            Term::Str(lock_h.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":rationale-artifact")),
                            Term::Str(lock_rationale_artifact),
                        ),
                        (
                            TermOrdKey(Term::symbol(":deps")),
                            Term::Vector(finalized.provenance),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            Ok(Value::data(Term::Map(m)))
        }

        "core/pkg-low::update" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::update".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg-low::update".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);
            let only_filter = match payload_pkg_only(payload) {
                Ok(xs) => xs.unwrap_or_default(),
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
            let mut l =
                match load_lock_model(lock_authority.as_deref_mut(), &lock_path, error_tok, op) {
                    Ok(x) => x,
                    Err(error) => return Ok(error),
                };

            let executed = match execute_workflow(
                identity_authority.as_deref_mut(),
                PkgResolutionWorkflow::Update,
                &only_filter,
                &lock_s,
                &l,
                store,
                refs,
                policy,
                pol,
                budget,
                timeout_ms,
                error_tok,
                op,
            ) {
                Ok(value) => value,
                Err(value) => return Ok(value),
            };
            let finalized = match finalize_workflow(
                identity_authority.as_deref_mut(),
                PkgResolutionWorkflow::Update,
                &only_filter,
                &lock_s,
                &mut l,
                executed,
                store,
                strict,
                error_tok,
                op,
            ) {
                Ok(value) => value,
                Err(value) => return Ok(value),
            };
            let update_rationale_artifact = l
                .artifacts
                .get("update_resolution_rationale")
                .cloned()
                .unwrap_or_default();
            let workspace_root = l
                .artifacts
                .get("root_workspace_snapshot")
                .cloned()
                .unwrap_or_default();

            let (bytes, lock_h) =
                match render_resolved_lock(lock_write_authority, &lock_s, &l, error_tok, op) {
                    Ok(result) => result,
                    Err(error) => return Ok(error),
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
            if let Err(e) = atomic_write_text(&lock_write_path, &bytes) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":lock-h")),
                Term::Str(lock_h.clone()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":updated")),
                Term::Int(finalized.updated_count.into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":selected-count")),
                Term::Int(finalized.selected_count.into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":rationale-count")),
                Term::Int((finalized.rationale.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":rationale")),
                Term::Vector(finalized.rationale),
            );
            m.insert(
                TermOrdKey(Term::symbol(":rationale-artifact")),
                Term::Str(update_rationale_artifact.clone()),
            );
            m.insert(TermOrdKey(Term::symbol(":strict")), Term::Bool(strict));
            m.insert(
                TermOrdKey(Term::symbol(":workspace-root")),
                Term::Str(workspace_root.clone()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":workspace-root")),
                            Term::Str(workspace_root),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock-h")),
                            Term::Str(lock_h.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":rationale-artifact")),
                            Term::Str(update_rationale_artifact),
                        ),
                        (
                            TermOrdKey(Term::symbol(":deps")),
                            Term::Vector(finalized.provenance),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            Ok(Value::data(Term::Map(m)))
        }

        "core/pkg-low::install" => install_verify::handle_pkg_install(
            payload,
            pol,
            policy,
            store,
            refs,
            lock_authority,
            identity_authority,
            budget,
            timeout_ms,
            error_tok,
            op,
        ),

        "core/pkg-low::verify" => install_verify::handle_pkg_verify(
            payload,
            pol,
            store,
            lock_authority,
            identity_authority,
            error_tok,
            op,
        ),

        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op-eff",
            format!("core/pkg-low dispatch received unsupported op_eff: {op_eff}"),
            Some(op),
        )),
    }
}

fn render_resolved_lock(
    authority: Option<&mut PkgLockWriteAuthority>,
    lock_path: &str,
    lock: &gc_pkg::GenesisLock,
    error_tok: SealId,
    op: &str,
) -> Result<(Vec<u8>, String), Value> {
    if let Some(authority) = authority {
        return match authority.write_model(lock_path, lock) {
            Ok(PkgLockWriteDecision::Write { bytes, lock_hash }) => Ok((bytes, lock_hash)),
            Ok(PkgLockWriteDecision::Error { code, message }) => {
                Err(mk_error(error_tok, &code, message, Some(op)))
            }
            Err(error) => Err(mk_error(
                error_tok,
                "core/pkg/authority-error",
                error.to_string(),
                Some(op),
            )),
        };
    }

    #[cfg(any(test, feature = "parity-oracle"))]
    {
        let bytes = lock.to_toml_canonical().into_bytes();
        let lock_hash = blake3::hash(&bytes).to_hex().to_string();
        Ok((bytes, lock_hash))
    }

    #[cfg(not(any(test, feature = "parity-oracle")))]
    Err(mk_error(
        error_tok,
        "core/pkg/authority-error",
        "lock and update require the artifact-loaded GenesisCode lock write authority".to_string(),
        Some(op),
    ))
}

fn load_lock_model(
    authority: Option<&mut PkgLockReadAuthority>,
    path: &std::path::Path,
    error_tok: SealId,
    op: &str,
) -> Result<gc_pkg::GenesisLock, Value> {
    if let Some(authority) = authority {
        let bytes = read_bounded_lock(path)
            .map_err(|message| mk_error(error_tok, "core/pkg/bad-lock", message, Some(op)))?;
        return match authority.read_model_toml(&bytes) {
            Ok(PkgLockModelDecision::Lock(lock)) => Ok(lock),
            Ok(PkgLockModelDecision::Error { code, message }) => {
                Err(mk_error(error_tok, &code, message, Some(op)))
            }
            Err(error) => Err(mk_error(
                error_tok,
                "core/pkg/authority-error",
                error.to_string(),
                Some(op),
            )),
        };
    }

    #[cfg(any(test, feature = "parity-oracle"))]
    {
        gc_pkg::GenesisLock::load(path)
            .map_err(|error| mk_error(error_tok, "core/pkg/bad-lock", error.to_string(), Some(op)))
    }

    #[cfg(not(any(test, feature = "parity-oracle")))]
    Err(mk_error(
        error_tok,
        "core/pkg/authority-error",
        "selfhost package lock model authority is unavailable".to_string(),
        Some(op),
    ))
}

fn validate_requirement_registry_alias(
    lock: &gc_pkg::GenesisLock,
    name: &str,
    req: &gc_pkg::Requirement,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    let Some(alias) = req.registry.as_deref() else {
        return Ok(());
    };
    if alias == "default" {
        return Ok(());
    }
    if lock.registries.contains_key(alias) {
        return Ok(());
    }
    let available = lock
        .registries
        .keys()
        .cloned()
        .map(Term::Str)
        .collect::<Vec<_>>();
    Err(mk_error_with_ctx(
        error_tok,
        "core/pkg/registry-not-found",
        format!("requirement `{name}` references unknown registry alias `{alias}`"),
        Some(op),
        Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":name")),
                    Term::Str(name.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":selector")),
                    Term::Str(req.selector.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":registry")),
                    Term::Str(alias.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":available-registries")),
                    Term::Vector(available),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_pkg_low_op_eff_returns_sealed_error_instead_of_panicking() {
        let mut budget = ArtifactBudgetState::default();
        let out = dispatch_resolution(
            "core/pkg-low::unsupported-op",
            &Term::Nil,
            None,
            &CapsPolicy::empty(),
            None,
            None,
            None,
            None,
            None,
            &mut budget,
            SealId(777),
            "core/pkg-low::lock",
            None,
        )
        .expect("dispatch should return value");

        match out {
            Value::Sealed { token, payload } => {
                assert_eq!(token, SealId(777));
                let Some(Term::Map(mm)) = payload.as_ref().as_data() else {
                    panic!("expected sealed error map payload");
                };
                let code = match mm.get(&TermOrdKey(Term::symbol(":error/code"))) {
                    Some(Term::Str(s)) => s.as_str(),
                    _ => "",
                };
                assert_eq!(code, "core/caps/unknown-op-eff");
            }
            other => panic!("expected sealed error value, got {}", other.debug_repr()),
        }
    }
}
