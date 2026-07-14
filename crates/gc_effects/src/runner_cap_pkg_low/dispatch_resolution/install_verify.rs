use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "pkg install dispatcher threads explicit capability/context handles for deterministic hydration and sealing"
)]
pub(super) fn handle_pkg_install(
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    budget: &mut ArtifactBudgetState,
    timeout_ms: Option<u64>,
    error_tok: SealId,
    op: &str,
) -> Result<Value, EffectsError> {
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
    let workspace_root = l.artifacts.get("root_workspace_snapshot").cloned();

    for (name, le) in &l.locked {
        let registry_alias = dependency_registry_alias(&l, name, le);
        let snapshot_hex = &le.snapshot;
        if !store.path_for(snapshot_hex).exists()
            && let (Some(req), Some(refs_db)) = (l.requirements.get(name), refs)
        {
            match resolve_requirement(
                store,
                refs_db,
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
                Ok(_) => {}
                Err(v) if is_not_found_error(&v) => {}
                Err(v) => return Ok(v),
            }
        }
        let snapshot_present = match try_hydrate_locked_hash(
            store,
            &l.registries,
            registry_alias,
            policy,
            pol,
            budget,
            timeout_ms,
            snapshot_hex,
            error_tok,
            op,
        ) {
            Ok(present) => present,
            Err(v) => return Ok(v),
        };
        if !snapshot_present {
            ok = false;
            missing_hashes.push(Term::Str(snapshot_hex.clone()));
            continue;
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
            let present = match try_hydrate_locked_hash(
                store,
                &l.registries,
                registry_alias,
                policy,
                pol,
                budget,
                timeout_ms,
                &h,
                error_tok,
                op,
            ) {
                Ok(present) => present,
                Err(v) => return Ok(v),
            };
            if !present {
                ok = false;
                missing_hashes.push(Term::Str(h));
                continue;
            }
            checked = checked.saturating_add(1);
        }

        if let Some(commit_hex) = &le.commit {
            let commit_present = match try_hydrate_locked_hash(
                store,
                &l.registries,
                registry_alias,
                policy,
                pol,
                budget,
                timeout_ms,
                commit_hex,
                error_tok,
                op,
            ) {
                Ok(present) => present,
                Err(v) => return Ok(v),
            };
            if !commit_present {
                ok = false;
                missing_hashes.push(Term::Str(commit_hex.clone()));
                continue;
            }
            checked = checked.saturating_add(1);

            if strict {
                if let Err(v) = hydrate_commit_closure(
                    store,
                    &l.registries,
                    registry_alias,
                    policy,
                    pol,
                    budget,
                    timeout_ms,
                    commit_hex,
                    error_tok,
                    op,
                ) {
                    return Ok(v);
                }
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
    }

    let deps_provenance =
        match locked_dependency_provenance(store, &l.locked, strict, error_tok, op) {
            Ok(v) => v,
            Err(v) => return Ok(v),
        };

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
    Ok(Value::data(Term::Map(m)))
}

fn dependency_registry_alias<'a>(
    lock: &'a gc_pkg::GenesisLock,
    name: &str,
    locked: &'a gc_pkg::LockedEntry,
) -> Option<&'a str> {
    locked.registry.as_deref().or_else(|| {
        lock.requirements
            .get(name)
            .and_then(|r| r.registry.as_deref())
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "locked hash hydration needs explicit policy/budget/timeout handles to preserve bounded network behavior"
)]
fn try_hydrate_locked_hash(
    store: &ArtifactStore,
    registries: &BTreeMap<String, String>,
    registry_alias: Option<&str>,
    policy: &CapsPolicy,
    op_pol: Option<&OpPolicy>,
    budget: &mut ArtifactBudgetState,
    timeout_ms: Option<u64>,
    hash: &str,
    error_tok: SealId,
    op: &str,
) -> Result<bool, Value> {
    match ensure_artifact_hash_available(
        store,
        registries,
        registry_alias,
        policy,
        op_pol,
        budget,
        timeout_ms,
        hash,
        error_tok,
        op,
    ) {
        Ok(()) => Ok(true),
        Err(v) if is_not_found_error(&v) => Ok(false),
        Err(v) => Err(v),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "commit closure hydration intentionally carries explicit policy and budgeting context through recursion"
)]
fn hydrate_commit_closure(
    store: &ArtifactStore,
    registries: &BTreeMap<String, String>,
    registry_alias: Option<&str>,
    policy: &CapsPolicy,
    op_pol: Option<&OpPolicy>,
    budget: &mut ArtifactBudgetState,
    timeout_ms: Option<u64>,
    commit_hex: &str,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    ensure_artifact_hash_available(
        store,
        registries,
        registry_alias,
        policy,
        op_pol,
        budget,
        timeout_ms,
        commit_hex,
        error_tok,
        op,
    )?;
    let commit_term = store_get_term(store, commit_hex).map_err(|e| {
        mk_error(
            error_tok,
            "core/pkg/bad-commit",
            format!("{commit_hex}: {e}"),
            Some(op),
        )
    })?;
    let commit = gc_vcs::Commit::from_term(&commit_term)
        .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
    if let Some(base_h) = commit.base.as_deref() {
        ensure_artifact_hash_available(
            store,
            registries,
            registry_alias,
            policy,
            op_pol,
            budget,
            timeout_ms,
            base_h,
            error_tok,
            op,
        )?;
    }
    ensure_artifact_hash_available(
        store,
        registries,
        registry_alias,
        policy,
        op_pol,
        budget,
        timeout_ms,
        &commit.patch,
        error_tok,
        op,
    )?;
    ensure_artifact_hash_available(
        store,
        registries,
        registry_alias,
        policy,
        op_pol,
        budget,
        timeout_ms,
        &commit.result,
        error_tok,
        op,
    )?;
    for ev_h in &commit.evidence {
        ensure_artifact_hash_available(
            store,
            registries,
            registry_alias,
            policy,
            op_pol,
            budget,
            timeout_ms,
            ev_h,
            error_tok,
            op,
        )?;
    }
    for at_h in &commit.attestations {
        ensure_artifact_hash_available(
            store,
            registries,
            registry_alias,
            policy,
            op_pol,
            budget,
            timeout_ms,
            at_h,
            error_tok,
            op,
        )?;
    }
    Ok(())
}

fn is_not_found_error(v: &Value) -> bool {
    let Value::Sealed { payload, .. } = v else {
        return false;
    };
    let Value::Data(t) = payload.as_ref() else {
        return false;
    };
    let Term::Map(mm) = t.as_ref() else {
        return false;
    };
    matches!(
        mm.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(Term::Str(code)) if code == "core/store/not-found"
    )
}

pub(super) fn handle_pkg_verify(
    payload: &Term,
    pol: Option<&OpPolicy>,
    store: Option<&ArtifactStore>,
    error_tok: SealId,
    op: &str,
) -> Result<Value, EffectsError> {
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
    Ok(Value::data(Term::Map(m)))
}
