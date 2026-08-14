use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "pkg install dispatcher threads explicit capability/context handles for deterministic hydration and sealing"
)]
pub(super) fn handle_pkg_install_parity(
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
    let l = match load_lock_model(lock_authority, &lock_path, error_tok, op) {
        Ok(x) => x,
        Err(error) => return Ok(error),
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
            let plan = match plan_requirement(
                identity_authority.as_deref_mut(),
                req,
                false,
                error_tok,
                op,
            ) {
                Ok(plan) => plan,
                Err(v) => return Ok(v),
            };
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
                plan,
                identity_authority.as_deref_mut(),
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
