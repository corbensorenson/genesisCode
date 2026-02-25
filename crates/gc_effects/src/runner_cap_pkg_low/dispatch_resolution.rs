use super::*;

#[path = "dispatch_resolution/install_verify.rs"]
mod install_verify;

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
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
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
            Ok(Value::Data(Term::Map(m)))
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
            let mut l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut out_locked: BTreeMap<String, gc_pkg::LockedEntry> = BTreeMap::new();
            for (name, req) in &l.requirements {
                if let Err(v) = validate_requirement_registry_alias(&l, name, req, error_tok, op) {
                    return Ok(v);
                }
                match resolve_requirement(
                    store,
                    refs,
                    &l.registries,
                    policy,
                    pol,
                    budget,
                    timeout_ms,
                    name,
                    req,
                    error_tok,
                    op,
                ) {
                    Ok(le) => {
                        out_locked.insert(name.clone(), le);
                    }
                    Err(err_val) => {
                        return Ok(annotate_requirement_resolution_error(err_val, name, req));
                    }
                }
            }

            if strict
                && let Err(v) = validate_locked_entries_strict(
                    store,
                    &l.requirements,
                    &out_locked,
                    true,
                    error_tok,
                    op,
                )
            {
                return Ok(v);
            }
            l.locked = out_locked;
            let workspace_root = match persist_workspace_root_snapshot(store, &l, error_tok, op) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };
            l.artifacts.insert(
                "root_workspace_snapshot".to_string(),
                workspace_root.clone(),
            );
            let deps_provenance =
                match locked_dependency_provenance(store, &l.locked, strict, error_tok, op) {
                    Ok(v) => v,
                    Err(v) => return Ok(v),
                };

            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
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
            if let Err(e) = atomic_write_text(&lock_write_path, bytes.as_bytes()) {
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
                Term::Int((l.locked.len() as i64).into()),
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
                            TermOrdKey(Term::symbol(":deps")),
                            Term::Vector(deps_provenance),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            Ok(Value::Data(Term::Map(m)))
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
                Ok(xs) => normalize_only_filter(xs),
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
            let mut l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut updated: u64 = 0;
            let mut rationale: Vec<Term> = Vec::new();
            let mut selected_count: u64 = 0;
            for (name, req) in &l.requirements {
                if let Err(v) = validate_requirement_registry_alias(&l, name, req, error_tok, op) {
                    return Ok(v);
                }
                if !only_filter.is_empty() && !only_filter.contains(name) {
                    rationale.push(update_rationale_term(
                        name,
                        ":skipped-unselected",
                        "not selected by --only filter",
                    ));
                    continue;
                }
                selected_count = selected_count.saturating_add(1);
                let should_update = req.update_policy == gc_pkg::UpdatePolicy::Auto
                    && matches!(
                        req.strategy,
                        gc_pkg::ResolutionStrategy::TrackRef
                            | gc_pkg::ResolutionStrategy::TagPolicy
                    );
                let has_existing = l.locked.contains_key(name);
                if !should_update && has_existing {
                    rationale.push(update_rationale_term(
                        name,
                        ":kept-existing",
                        "update_policy=manual and locked entry already present",
                    ));
                    continue;
                }
                let previous = l.locked.get(name).cloned();
                match resolve_requirement(
                    store,
                    refs,
                    &l.registries,
                    policy,
                    pol,
                    budget,
                    timeout_ms,
                    name,
                    req,
                    error_tok,
                    op,
                ) {
                    Ok(le) => {
                        let changed = previous
                            .as_ref()
                            .map(|old| !locked_entry_eq(old, &le))
                            .unwrap_or(true);
                        l.locked.insert(name.clone(), le);
                        if changed {
                            updated = updated.saturating_add(1);
                            rationale.push(update_rationale_term(
                                name,
                                ":updated",
                                if has_existing {
                                    "resolved new lock entry for selected dependency"
                                } else {
                                    "resolved missing locked entry"
                                },
                            ));
                        } else {
                            rationale.push(update_rationale_term(
                                name,
                                ":no-change",
                                "resolved dependency equals existing lock entry",
                            ));
                        }
                    }
                    Err(err_val) => {
                        return Ok(annotate_requirement_resolution_error(err_val, name, req));
                    }
                }
            }
            if !only_filter.is_empty() {
                for selected in &only_filter {
                    if !l.requirements.contains_key(selected) {
                        rationale.push(update_rationale_term(
                            selected,
                            ":missing-requirement",
                            "selected dependency is not present in lock requirements",
                        ));
                    }
                }
            }
            if strict
                && let Err(v) = validate_locked_entries_strict(
                    store,
                    &l.requirements,
                    &l.locked,
                    true,
                    error_tok,
                    op,
                )
            {
                return Ok(v);
            }
            let workspace_root = match persist_workspace_root_snapshot(store, &l, error_tok, op) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };
            l.artifacts.insert(
                "root_workspace_snapshot".to_string(),
                workspace_root.clone(),
            );
            let deps_provenance =
                match locked_dependency_provenance(store, &l.locked, strict, error_tok, op) {
                    Ok(v) => v,
                    Err(v) => return Ok(v),
                };

            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
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
            if let Err(e) = atomic_write_text(&lock_write_path, bytes.as_bytes()) {
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
                Term::Int((updated as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":selected-count")),
                Term::Int((selected_count as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":rationale-count")),
                Term::Int((rationale.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":rationale")),
                Term::Vector(rationale),
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
                            TermOrdKey(Term::symbol(":deps")),
                            Term::Vector(deps_provenance),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::install" => install_verify::handle_pkg_install(
            payload, pol, policy, store, refs, budget, timeout_ms, error_tok, op,
        ),

        "core/pkg-low::verify" => {
            install_verify::handle_pkg_verify(payload, pol, store, error_tok, op)
        }

        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op-eff",
            format!("core/pkg-low dispatch received unsupported op_eff: {op_eff}"),
            Some(op),
        )),
    }
}

fn normalize_only_filter(raw: Option<Vec<String>>) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    if let Some(xs) = raw {
        for x in xs {
            let trimmed = x.trim();
            if !trimmed.is_empty() {
                out.insert(trimmed.to_string());
            }
        }
    }
    out
}

fn locked_entry_eq(a: &gc_pkg::LockedEntry, b: &gc_pkg::LockedEntry) -> bool {
    a.commit == b.commit
        && a.snapshot == b.snapshot
        && a.registry == b.registry
        && a.source_selector == b.source_selector
        && a.resolved_ref == b.resolved_ref
        && a.exports_hash == b.exports_hash
        && a.environment_fingerprint == b.environment_fingerprint
}

fn update_rationale_term(name: &str, action_sym: &str, reason: &str) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str(name.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":action")),
                Term::symbol(action_sym),
            ),
            (
                TermOrdKey(Term::symbol(":reason")),
                Term::Str(reason.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    )
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

fn annotate_requirement_resolution_error(
    err: Value,
    name: &str,
    req: &gc_pkg::Requirement,
) -> Value {
    let Value::Sealed { token, payload } = err else {
        return err;
    };
    let Value::Data(Term::Map(mut mm)) = *payload else {
        return Value::Sealed { token, payload };
    };
    let existing_ctx = mm
        .get(&TermOrdKey(Term::symbol(":error/context")))
        .cloned()
        .unwrap_or(Term::Nil);
    let mut ctx = BTreeMap::new();
    ctx.insert(
        TermOrdKey(Term::symbol(":name")),
        Term::Str(name.to_string()),
    );
    ctx.insert(
        TermOrdKey(Term::symbol(":selector")),
        Term::Str(req.selector.clone()),
    );
    ctx.insert(
        TermOrdKey(Term::symbol(":strategy")),
        Term::symbol(format!(":{}", req.strategy.as_str())),
    );
    ctx.insert(
        TermOrdKey(Term::symbol(":registry")),
        req.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
    );
    ctx.insert(TermOrdKey(Term::symbol(":inner")), existing_ctx);
    mm.insert(TermOrdKey(Term::symbol(":error/context")), Term::Map(ctx));
    Value::Sealed {
        token,
        payload: Box::new(Value::Data(Term::Map(mm))),
    }
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
            &mut budget,
            SealId(777),
            "core/pkg-low::lock",
            None,
        )
        .expect("dispatch should return value");

        match out {
            Value::Sealed { token, payload } => {
                assert_eq!(token, SealId(777));
                let Value::Data(Term::Map(mm)) = *payload else {
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
