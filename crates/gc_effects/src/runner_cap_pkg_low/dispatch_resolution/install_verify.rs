use super::*;

#[cfg(any(test, feature = "parity-oracle"))]
#[path = "install_verify/parity.rs"]
mod parity;

#[cfg(any(test, feature = "parity-oracle"))]
use parity::handle_pkg_install_parity;

#[expect(
    clippy::too_many_arguments,
    reason = "install mechanisms retain explicit policy and resource handles"
)]
pub(super) fn handle_pkg_install(
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    lock_authority: Option<&mut PkgLockReadAuthority>,
    mut identity_authority: Option<&mut PkgResolutionIdentityAuthority>,
    budget: &mut ArtifactBudgetState,
    timeout_ms: Option<u64>,
    error_tok: SealId,
    op: &str,
) -> Result<Value, EffectsError> {
    let Some(authority) = identity_authority.as_deref_mut() else {
        #[cfg(any(test, feature = "parity-oracle"))]
        {
            return handle_pkg_install_parity(
                payload,
                pol,
                policy,
                store,
                refs,
                lock_authority,
                None,
                budget,
                timeout_ms,
                error_tok,
                op,
            );
        }
        #[cfg(not(any(test, feature = "parity-oracle")))]
        {
            return Ok(mk_error(
                error_tok,
                "core/pkg/authority-error",
                "package install requires the artifact-loaded GenesisCode install authority"
                    .to_string(),
                Some(op),
            ));
        }
    };
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/pkg-low::install".to_string())
    })?;
    let lock_s = match payload_pkg_lock(payload) {
        Ok(value) => value,
        Err(error) => {
            return Ok(mk_error(error_tok, "core/pkg/bad-payload", error, Some(op)));
        }
    };
    let frozen = payload_pkg_bool(payload, ":frozen").unwrap_or(false);
    let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);
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
    let lock = match load_lock_model(lock_authority, &lock_path, error_tok, op) {
        Ok(lock) => lock,
        Err(error) => return Ok(error),
    };
    let model = crate::pkg_lock_write_authority::lock_model_payload(&lock_s, &lock)
        .map_err(|error| EffectsError::Log(error.to_string()))?;
    let plan = match authority
        .plan_install(model, frozen, strict, refs.is_some())
        .map_err(|error| EffectsError::Log(error.to_string()))?
    {
        PkgInstallPlanDecision::Admit(plan) => plan,
        PkgInstallPlanDecision::FrozenMissing(missing) => {
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
        PkgInstallPlanDecision::Error { code, message } => {
            return Ok(mk_error(error_tok, &code, message, Some(op)));
        }
    };

    let mut observations = Vec::with_capacity(plan.steps.len());
    for step in &plan.steps {
        let initially_present = store.path_for(&step.snapshot).exists();
        let resolution = if initially_present {
            PkgInstallResolution::NotNeeded
        } else if step.resolve_if_missing {
            let requirement = lock.requirements.get(&step.name).ok_or_else(|| {
                EffectsError::Log(format!(
                    "install authority planned resolution for missing requirement {}",
                    step.name
                ))
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log(
                    "install authority planned resolution without refs database".to_string(),
                )
            })?;
            let resolution_plan =
                match plan_requirement(Some(authority), requirement, false, error_tok, op) {
                    Ok(plan) => plan,
                    Err(value) => return Ok(value),
                };
            match resolve_requirement(
                store,
                refs,
                &lock.registries,
                policy,
                pol,
                budget,
                timeout_ms,
                &step.name,
                requirement,
                resolution_plan,
                Some(authority),
                error_tok,
                op,
            ) {
                Ok(_) => PkgInstallResolution::Resolved,
                Err(value) if is_not_found_error(&value) => PkgInstallResolution::NotFound,
                Err(value) => return Ok(value),
            }
        } else {
            PkgInstallResolution::Unavailable
        };

        let snapshot_present = match try_hydrate_locked_hash(
            store,
            &lock.registries,
            step.registry.as_deref(),
            policy,
            pol,
            budget,
            timeout_ms,
            &step.snapshot,
            error_tok,
            op,
        ) {
            Ok(present) => present,
            Err(value) => return Ok(value),
        };
        if !snapshot_present {
            observations.push(PkgInstallObservation {
                closure_checked: 0,
                commit_present: None,
                hashes: vec![PkgInstallHashObservation {
                    hash: step.snapshot.clone(),
                    present: false,
                }],
                initially_present,
                name: step.name.clone(),
                resolution,
            });
            continue;
        }

        let snapshot_term = match store_get_term(store, &step.snapshot) {
            Ok(term) => term,
            Err(error) => {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-snapshot",
                    error.to_string(),
                    Some(op),
                ));
            }
        };
        let snapshot = match gc_vcs::Snapshot::from_term(&snapshot_term) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-snapshot",
                    error.to_string(),
                    Some(op),
                ));
            }
        };
        let mut hashes = vec![step.snapshot.clone()];
        hashes.extend(snapshot.shallow_refs());
        hashes.sort();
        hashes.dedup();
        if hashes.first() != Some(&step.snapshot) {
            hashes.retain(|hash| hash != &step.snapshot);
            hashes.insert(0, step.snapshot.clone());
        }
        let mut hash_observations = Vec::with_capacity(hashes.len());
        for hash in hashes {
            let present = match try_hydrate_locked_hash(
                store,
                &lock.registries,
                step.registry.as_deref(),
                policy,
                pol,
                budget,
                timeout_ms,
                &hash,
                error_tok,
                op,
            ) {
                Ok(present) => present,
                Err(value) => return Ok(value),
            };
            hash_observations.push(PkgInstallHashObservation { hash, present });
        }

        let mut closure_checked = 0;
        let commit_present = if let Some(commit) = step.commit.as_deref() {
            let present = match try_hydrate_locked_hash(
                store,
                &lock.registries,
                step.registry.as_deref(),
                policy,
                pol,
                budget,
                timeout_ms,
                commit,
                error_tok,
                op,
            ) {
                Ok(present) => present,
                Err(value) => return Ok(value),
            };
            if present && strict {
                if let Err(value) = hydrate_commit_closure(
                    store,
                    &lock.registries,
                    step.registry.as_deref(),
                    policy,
                    pol,
                    budget,
                    timeout_ms,
                    commit,
                    error_tok,
                    op,
                ) {
                    return Ok(value);
                }
                closure_checked = match validate_commit_artifact_closure(
                    store,
                    &step.name,
                    &step.snapshot,
                    commit,
                    true,
                    error_tok,
                    op,
                ) {
                    Ok(count) => count,
                    Err(value) => return Ok(value),
                };
            }
            Some(present)
        } else {
            None
        };
        observations.push(PkgInstallObservation {
            closure_checked,
            commit_present,
            hashes: hash_observations,
            initially_present,
            name: step.name.clone(),
            resolution,
        });
    }

    let commit_observations = super::workflow::commit_observations(store, &lock.locked);
    match authority
        .finalize_install(&plan, &observations, commit_observations)
        .map_err(|error| EffectsError::Log(error.to_string()))?
    {
        Ok(finalized) => Ok(Value::data(finalized.report)),
        Err((code, message)) => Ok(mk_error(error_tok, &code, message, Some(op))),
    }
}

#[cfg(any(test, feature = "parity-oracle"))]
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
    lock_authority: Option<&mut PkgLockReadAuthority>,
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
    let l = match load_lock_model(lock_authority, &lock_path, error_tok, op) {
        Ok(x) => x,
        Err(error) => return Ok(error),
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
