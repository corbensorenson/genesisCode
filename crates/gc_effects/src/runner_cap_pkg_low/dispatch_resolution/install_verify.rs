use super::*;

pub(super) fn handle_pkg_install(
    payload: &Term,
    pol: Option<&OpPolicy>,
    store: Option<&ArtifactStore>,
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
    let deps_provenance = match locked_dependency_provenance(store, &l.locked, false, error_tok, op)
    {
        Ok(v) => v,
        Err(v) => return Ok(v),
    };
    let workspace_root = l.artifacts.get("root_workspace_snapshot").cloned();

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
    Ok(Value::Data(Term::Map(m)))
}
