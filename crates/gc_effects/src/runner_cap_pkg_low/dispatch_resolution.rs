use super::*;

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
    let _ = (policy, budget, timeout_ms);
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
                match resolve_requirement(store, refs, name, req, error_tok, op) {
                    Ok(le) => {
                        out_locked.insert(name.clone(), le);
                    }
                    Err(err_val) => return Ok(err_val),
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
            for (name, req) in &l.requirements {
                let should_update = req.update_policy == gc_pkg::UpdatePolicy::Auto
                    && matches!(
                        req.strategy,
                        gc_pkg::ResolutionStrategy::TrackRef
                            | gc_pkg::ResolutionStrategy::TagPolicy
                    );
                if !should_update && l.locked.contains_key(name) {
                    continue;
                }
                match resolve_requirement(store, refs, name, req, error_tok, op) {
                    Ok(le) => {
                        l.locked.insert(name.clone(), le);
                        updated = updated.saturating_add(1);
                    }
                    Err(err_val) => return Ok(err_val),
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

        "core/pkg-low::install" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::install".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let frozen = payload_pkg_bool(payload, ":frozen").unwrap_or(false);
            let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);

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
            if frozen {
                let missing = l.requirements_missing_locks();
                if !missing.is_empty() {
                    return Ok(mk_error_with_ctx(
                        error_tok,
                        "core/pkg/not-locked",
                        "lock is missing locked entries".to_string(),
                        Some(op),
                        Term::Map(
                            [(
                                TermOrdKey(Term::symbol(":missing")),
                                Term::Vector(missing.into_iter().map(Term::Str).collect()),
                            )]
                            .into_iter()
                            .collect(),
                        ),
                    ));
                }
            }

            let mut ok = true;
            let mut missing_hashes: Vec<Term> = Vec::new();
            let mut checked: u64 = 0;
            let deps_provenance =
                match locked_dependency_provenance(store, &l.locked, false, error_tok, op) {
                    Ok(v) => v,
                    Err(v) => return Ok(v),
                };
            let workspace_root = l.artifacts.get("root_workspace_snapshot").cloned();

            for (name, le) in &l.locked {
                // Snapshot must exist and be well-formed.
                let snapshot_hex = &le.snapshot;
                if !store.path_for(snapshot_hex).exists() {
                    ok = false;
                    missing_hashes.push(Term::Str(snapshot_hex.clone()));
                    continue;
                }
                if store.verify_hex(snapshot_hex).is_err() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {snapshot_hex}"),
                        Some(op),
                    ));
                }
                let snap_term = match store_get_term(store, snapshot_hex) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let snap = match gc_vcs::Snapshot::from_term(&snap_term) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };

                // Shallow closure: snapshot + module artifacts.
                let mut hashes: Vec<String> = Vec::new();
                hashes.push(snapshot_hex.clone());
                hashes.extend(snap.shallow_refs());
                hashes.sort();
                hashes.dedup();

                for h in hashes {
                    if !store.path_for(&h).exists() {
                        ok = false;
                        missing_hashes.push(Term::Str(h));
                        continue;
                    }
                    if store.verify_hex(&h).is_err() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("artifact store corruption: {h}"),
                            Some(op),
                        ));
                    }
                    checked = checked.saturating_add(1);
                }

                if strict && let Some(commit_hex) = &le.commit {
                    match validate_commit_artifact_closure(
                        store,
                        name,
                        snapshot_hex,
                        commit_hex,
                        true,
                        error_tok,
                        op,
                    ) {
                        Ok(n) => checked = checked.saturating_add(n),
                        Err(v) => return Ok(v),
                    }
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":checked")),
                Term::Int((checked as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":missing")),
                Term::Vector(missing_hashes),
            );
            m.insert(
                TermOrdKey(Term::symbol(":workspace-root")),
                workspace_root.clone().map(Term::Str).unwrap_or(Term::Nil),
            );
            m.insert(
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":workspace-root")),
                            workspace_root.map(Term::Str).unwrap_or(Term::Nil),
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

        "core/pkg-low::verify" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::verify".to_string())
            })?;
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

            let mut ok = true;
            let mut missing_hashes: Vec<Term> = Vec::new();
            let mut checked: u64 = 0;

            for (name, le) in &l.locked {
                let snapshot_hex = &le.snapshot;
                if !store.path_for(snapshot_hex).exists() {
                    ok = false;
                    missing_hashes.push(Term::Str(snapshot_hex.clone()));
                    continue;
                }
                if store.verify_hex(snapshot_hex).is_err() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {snapshot_hex}"),
                        Some(op),
                    ));
                }
                let snap_term = match store_get_term(store, snapshot_hex) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let snap = match gc_vcs::Snapshot::from_term(&snap_term) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let mut hashes: Vec<String> = Vec::new();
                hashes.push(snapshot_hex.clone());
                hashes.extend(snap.shallow_refs());
                hashes.sort();
                hashes.dedup();
                for h in hashes {
                    if !store.path_for(&h).exists() {
                        ok = false;
                        missing_hashes.push(Term::Str(h));
                        continue;
                    }
                    if store.verify_hex(&h).is_err() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("artifact store corruption: {h}"),
                            Some(op),
                        ));
                    }
                    checked = checked.saturating_add(1);
                }

                if let Some(commit_hex) = &le.commit {
                    match validate_commit_artifact_closure(
                        store,
                        name,
                        snapshot_hex,
                        commit_hex,
                        true,
                        error_tok,
                        op,
                    ) {
                        Ok(n) => checked = checked.saturating_add(n),
                        Err(v) => return Ok(v),
                    }
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":checked")),
                Term::Int((checked as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":missing")),
                Term::Vector(missing_hashes),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op-eff",
            format!("core/pkg-low dispatch received unsupported op_eff: {op_eff}"),
            Some(op),
        )),
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
