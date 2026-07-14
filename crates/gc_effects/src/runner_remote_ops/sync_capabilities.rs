#[expect(
    clippy::too_many_arguments,
    reason = "sync pull needs explicit policy/store/log context to keep effect behavior auditable"
)]
pub(super) fn capability_sync_pull(
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
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/sync::pull".to_string())
    })?;
    let refs =
        refs.ok_or_else(|| EffectsError::Log("missing refs db for core/sync::pull".to_string()))?;

    let remote_s = match payload_sync_remote(payload) {
        Ok(s) => s,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    let depth = payload_sync_depth(payload).unwrap_or(0);
    let force = payload_sync_force(payload).unwrap_or(false);
    let refnames = match payload_sync_refs(payload) {
        Ok(rs) => rs,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    let roots = match payload_sync_roots(payload) {
        Ok(rs) => rs,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    if refnames.is_empty() && roots.is_empty() {
        return Ok(mk_error(
            error_tok,
            "core/sync/bad-payload",
            "pull requires :refs and/or :roots".to_string(),
            Some(op),
        ));
    }

    let sp = match sync_policy_from_op(pol) {
        Ok(p) => p,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let base = match sync_normalize_and_check_remote(&sp, &remote_s) {
        Ok(b) => b,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/remote-denied", e, Some(op)));
        }
    };
    let auth = match sync_registry_auth(&sp) {
        Ok(a) => a,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let client = match gc_registry::RegistryClient::new_with_auth(
        &base,
        timeout_ms.map(std::time::Duration::from_millis),
        auth,
    ) {
        Ok(c) => c,
        Err(e) => {
            let code = registry_error_code(&e, "core/sync/remote-auth");
            return Ok(mk_error(error_tok, code, format!("{e}"), Some(op)));
        }
    };
    let mut pulled: u64 = 0;
    let mut already: u64 = 0;
    let mut heads: Vec<Term> = Vec::new();

    for h in &roots {
        let mut stats = SyncPullStats {
            pulled: &mut pulled,
            already: &mut already,
            store_written_bytes: &mut budget.store_written_bytes,
            store_max_run_bytes: policy.store.max_run_bytes,
            error_tok,
            op,
            transfer_workers: sp.transfer_workers,
            max_artifact_bytes: sp.max_artifact_bytes,
            max_batch_bytes: sp.max_batch_bytes,
        };
        match sync_pull_closure(&client, store, h, depth, &mut stats) {
            Ok(()) => {}
            Err(v) => return Ok(v),
        }
    }

    for rname in &refnames {
        let h = match client.refs_get(rname) {
            Ok(Some(h)) => h,
            Ok(None) => {
                return Ok(mk_error(
                    error_tok,
                    "core/sync/ref-not-found",
                    format!("remote ref not found: {rname}"),
                    Some(op),
                ));
            }
            Err(e) => {
                let code = registry_error_code(&e, "core/sync/remote-auth");
                return Ok(mk_error(error_tok, code, format!("{e}"), Some(op)));
            }
        };
        let mut stats = SyncPullStats {
            pulled: &mut pulled,
            already: &mut already,
            store_written_bytes: &mut budget.store_written_bytes,
            store_max_run_bytes: policy.store.max_run_bytes,
            error_tok,
            op,
            transfer_workers: sp.transfer_workers,
            max_artifact_bytes: sp.max_artifact_bytes,
            max_batch_bytes: sp.max_batch_bytes,
        };
        match sync_pull_closure(&client, store, &h, depth, &mut stats) {
            Ok(()) => {}
            Err(v) => return Ok(v),
        }

        let cur = refs.get(rname)?;
        if !force
            && let Some(curh) = &cur
            && curh != &h
        {
            return Ok(mk_error_with_ctx(
                error_tok,
                "core/refs/conflict",
                "local ref differs; use force to overwrite".to_string(),
                Some(op),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":refs/name")),
                            Term::Str(rname.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":refs/current")),
                            cur.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":refs/remote")),
                            Term::Str(h.clone()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ));
        }
        let _ = refs.set(rname, Some(&h), None)?;

        heads.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":name")), Term::Str(rname.clone())),
                (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
            ]
            .into_iter()
            .collect(),
        ));
    }

    heads.sort_by_cached_key(print_term);

    let mut m = BTreeMap::new();
    m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(base));
    m.insert(
        TermOrdKey(Term::symbol(":pulled")),
        Term::Int((pulled as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":present")),
        Term::Int((already as i64).into()),
    );
    m.insert(TermOrdKey(Term::symbol(":heads")), Term::Vector(heads));
    Ok(Value::data(Term::Map(m)))
}

pub(super) fn capability_sync_push(
    payload: &Term,
    pol: Option<&OpPolicy>,
    store: Option<&ArtifactStore>,
    error_tok: SealId,
    op: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/sync::push".to_string())
    })?;

    let remote_s = match payload_sync_remote(payload) {
        Ok(s) => s,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    let depth = payload_sync_depth(payload).unwrap_or(0);
    let roots = match payload_sync_roots(payload) {
        Ok(rs) => rs,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    if roots.is_empty() {
        return Ok(mk_error(
            error_tok,
            "core/sync/bad-payload",
            "push requires :roots".to_string(),
            Some(op),
        ));
    }
    let set_refs = match payload_sync_set_refs(payload) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
        }
    };
    for sr in &set_refs {
        if let Err(v) = local_refs_validate_policy_gate(
            store,
            &sr.name,
            Some(&sr.hash),
            &sr.policy,
            error_tok,
            op,
        ) {
            return Ok(v);
        }
    }

    let sp = match sync_policy_from_op(pol) {
        Ok(p) => p,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let base = match sync_normalize_and_check_remote(&sp, &remote_s) {
        Ok(b) => b,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/sync/remote-denied", e, Some(op)));
        }
    };
    let auth = match sync_registry_auth(&sp) {
        Ok(a) => a,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let client = match gc_registry::RegistryClient::new_with_auth(
        &base,
        timeout_ms.map(std::time::Duration::from_millis),
        auth,
    ) {
        Ok(c) => c,
        Err(e) => {
            let code = registry_error_code(&e, "core/sync/remote-auth");
            return Ok(mk_error(error_tok, code, format!("{e}"), Some(op)));
        }
    };
    let remote_max_chunk_bytes = match client.ping() {
        Ok(p) => p
            .max_chunk_bytes
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n > 0),
        Err(_) => None,
    };

    let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for h in &roots {
        match sync_closure_local(store, h, depth, &mut all, error_tok, op) {
            Ok(()) => {}
            Err(v) => return Ok(v),
        }
    }
    let hashes: Vec<String> = all.into_iter().collect();

    let mut missing: Vec<String> = Vec::new();
    let mut present: u64 = 0;
    let has_chunks: Vec<Vec<String>> = hashes.chunks(512).map(|chunk| chunk.to_vec()).collect();
    let has_results = sync_parallel_store_has_chunks(&client, &has_chunks, sp.transfer_workers);
    for (chunk_i, chunk) in hashes.chunks(512).enumerate() {
        let mp = match &has_results[chunk_i] {
            Ok(m) => m,
            Err(e) => {
                let code = registry_error_code(e, "core/sync/remote-auth");
                return Ok(mk_error(error_tok, code, format!("{e}"), Some(op)));
            }
        };
        for h in chunk {
            match mp.get(h) {
                Some(true) => present = present.saturating_add(1),
                _ => missing.push(h.clone()),
            }
        }
    }
    missing.sort();
    missing.dedup();

    let upload_results = sync_parallel_upload_missing(
        &client,
        store,
        &missing,
        sp.transfer_workers,
        remote_max_chunk_bytes,
    );
    let mut uploaded: u64 = 0;
    for r in upload_results {
        match r {
            Ok(()) => uploaded = uploaded.saturating_add(1),
            Err(e) => {
                let (code, msg) = if e.starts_with("store-read:") {
                    ("core/store/not-found", e)
                } else if e.starts_with("auth error:") {
                    ("core/sync/remote-auth", e)
                } else {
                    ("core/sync/remote-error", e)
                };
                return Ok(mk_error(error_tok, code, msg, Some(op)));
            }
        }
    }

    let mut refs_updated: u64 = 0;
    if !set_refs.is_empty() {
        let mut set_refs_sorted = set_refs;
        set_refs_sorted.sort_by(|a, b| a.name.cmp(&b.name));
        for sr in &set_refs_sorted {
            let req = gc_registry::RefsSetReq {
                name: &sr.name,
                hash: &sr.hash,
                policy: &sr.policy,
                expected_old: sr.expected_old.as_deref(),
            };
            match client.refs_set(&req) {
                Ok(r) => {
                    if !r.ok {
                        return Ok(mk_error(
                            error_tok,
                            "core/sync/refs-set-failed",
                            "remote refs/set returned ok=false".to_string(),
                            Some(op),
                        ));
                    }
                    refs_updated = refs_updated.saturating_add(1);
                }
                Err(e) => {
                    let code = registry_error_code(&e, "core/sync/remote-auth");
                    return Ok(mk_error(error_tok, code, format!("{e}"), Some(op)));
                }
            }
        }
    }

    let mut m = BTreeMap::new();
    m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(base));
    m.insert(
        TermOrdKey(Term::symbol(":total")),
        Term::Int((hashes.len() as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":present")),
        Term::Int((present as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":uploaded")),
        Term::Int((uploaded as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":refs-updated")),
        Term::Int((refs_updated as i64).into()),
    );
    Ok(Value::data(Term::Map(m)))
}

pub(super) fn sync_closure_local(
    store: &ArtifactStore,
    root: &str,
    depth: u64,
    out: &mut std::collections::BTreeSet<String>,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};
    let mut q: VecDeque<(String, u64)> = VecDeque::new();
    q.push_back((root.to_string(), depth));
    let mut seen: HashSet<String> = HashSet::new();
    let mut obj_count: u64 = 0;

    while let Some((h, dleft)) = q.pop_front() {
        if !seen.insert(h.clone()) {
            continue;
        }
        obj_count = obj_count.saturating_add(1);
        if obj_count > 50_000 {
            return Err(mk_error(
                error_tok,
                "core/sync/too-many-objects",
                "closure exceeded 50k objects".to_string(),
                Some(op),
            ));
        }
        if !store.path_for(&h).exists() {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {h}"),
                Some(op),
            ));
        }
        if store.verify_hex(&h).is_err() {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact store corruption: {h}"),
                Some(op),
            ));
        }
        out.insert(h.clone());

        let t = match store_get_term(store, &h) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Ok(c) = gc_vcs::Commit::from_term(&t) {
            if let Some(b) = c.base {
                q.push_back((b, dleft));
            }
            q.push_back((c.patch, dleft));
            q.push_back((c.result, dleft));
            for x in c.evidence {
                q.push_back((x, dleft));
            }
            for x in c.attestations {
                q.push_back((x, dleft));
            }
            if dleft > 0 {
                for p in c.parents {
                    q.push_back((p, dleft - 1));
                }
            }
            continue;
        }
        if let Ok(p) = gc_vcs::Patch::from_term(&t) {
            for x in p.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(e) = gc_vcs::Evidence::from_term(&t) {
            for x in e.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(c) = gc_vcs::Conflict::from_term(&t) {
            for x in c.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(s) = gc_vcs::Snapshot::from_term(&t) {
            for x in s.shallow_refs() {
                q.push_back((x, dleft));
            }
        }
    }
    Ok(())
}
