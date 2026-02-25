use super::*;

pub(super) struct GpkDispatchCtx<'a> {
    pub(super) pol: Option<&'a OpPolicy>,
    pub(super) policy: &'a CapsPolicy,
    pub(super) store: Option<&'a ArtifactStore>,
    pub(super) refs: Option<&'a RefsDb>,
    pub(super) budget: &'a mut ArtifactBudgetState,
    pub(super) error_tok: SealId,
    pub(super) op: &'a str,
}

pub(super) fn handle_gpk_export(
    payload: &Term,
    ctx: &mut GpkDispatchCtx<'_>,
) -> Result<Value, EffectsError> {
    let pol = ctx.pol;
    let store = ctx.store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/gpk-low::export".to_string())
    })?;
    let refs = ctx
        .refs
        .ok_or_else(|| EffectsError::Log("missing refs db for core/gpk-low::export".to_string()));
    let error_tok = ctx.error_tok;
    let op = ctx.op;
    let root_spec = match payload_gpk_root(payload) {
        Ok(s) => s,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/bad-payload",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    let out_path_s = match payload_gpk_out(payload) {
        Ok(s) => s,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/bad-payload",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    let base_dir = effective_base_dir(pol)?;
    let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
    let out_path = sandbox_path_write(&base_dir, &out_path_s, create_dirs)?;

    let mode = match payload_gpk_mode(payload) {
        Ok(Some(m)) => m,
        Ok(None) => ":shallow".to_string(),
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/bad-payload",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    let mode = match mode.as_str() {
        ":shallow" => GpkMode::Shallow,
        ":full" => GpkMode::Full,
        other => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/bad-payload",
                format!("unsupported :mode {other}"),
                Some(op),
            ));
        }
    };
    let depth = payload_gpk_depth(payload).unwrap_or(0);
    let include_evidence = match payload_gpk_include_evidence(payload) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/bad-payload",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    let include_deps = match payload_gpk_include_deps(payload) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/bad-payload",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    let embed_refnames = match payload_gpk_refs(payload) {
        Ok(xs) => xs,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/bad-payload",
                format!("{e}"),
                Some(op),
            ));
        }
    };

    let resolved_root = match resolve_gpk_root_for_export(
        store,
        refs.as_ref().ok().copied(),
        &root_spec,
        mode,
        error_tok,
        op,
    ) {
        Ok(h) => h,
        Err(v) => return Ok(v),
    };

    let root_term = match store_get_term(store, &resolved_root) {
        Ok(t) => t,
        Err(_) => {
            return Ok(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {resolved_root}"),
                Some(op),
            ));
        }
    };

    if mode == GpkMode::Shallow
        && let Err(e) = gc_vcs::Snapshot::from_term(&root_term)
    {
        return Ok(mk_error(
            error_tok,
            "core/gpk/bad-root",
            format!("{e}"),
            Some(op),
        ));
    }

    let root_snapshot_for_locked_deps = match mode {
        GpkMode::Shallow => Some(resolved_root.clone()),
        GpkMode::Full => {
            if let Ok(c) = gc_vcs::Commit::from_term(&root_term) {
                Some(c.result)
            } else if gc_vcs::Snapshot::from_term(&root_term).is_ok() {
                Some(resolved_root.clone())
            } else {
                None
            }
        }
    };
    let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    match gpk_export_closure_local(
        store,
        &resolved_root,
        GpkClosureOptions {
            depth: if mode == GpkMode::Shallow { 0 } else { depth },
            mode,
            include_evidence,
            include_deps,
            root_snapshot_for_locked_deps: root_snapshot_for_locked_deps.as_deref(),
        },
        &mut all,
        error_tok,
        op,
    ) {
        Ok(()) => {}
        Err(v) => return Ok(v),
    }
    let hashes: Vec<String> = all.into_iter().collect();

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for h in &hashes {
        if !store.path_for(h).exists() {
            return Ok(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {h}"),
                Some(op),
            ));
        }
        if store.verify_hex(h).is_err() {
            return Ok(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact store corruption: {h}"),
                Some(op),
            ));
        }
        let bytes = match store.get_bytes(h) {
            Ok(b) => b,
            Err(e) => {
                return Ok(mk_error(
                    error_tok,
                    "core/store/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        entries.push((h.clone(), bytes));
    }

    let root_b = match gc_vcs::hex_to_bytes32(&resolved_root) {
        Ok(b) => b,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/gpk/bad-root", e, Some(op)));
        }
    };

    let mut refs_section: Vec<(String, String)> = Vec::new();
    let bundle_version: u32 = if embed_refnames.is_empty() { 1 } else { 2 };
    if bundle_version == 2 {
        let refs = match refs {
            Ok(r) => r,
            Err(e) => {
                return Ok(mk_error(
                    error_tok,
                    "core/gpk/missing-refs-db",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        for name in &embed_refnames {
            let cur = match refs.get(name) {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/refs-io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let Some(h) = cur else {
                return Ok(mk_error(
                    error_tok,
                    "core/gpk/ref-not-found",
                    format!("ref not found: {name}"),
                    Some(op),
                ));
            };
            refs_section.push((name.clone(), h));
        }
        refs_section.sort_by(|a, b| a.0.cmp(&b.0));
        refs_section.dedup_by(|a, b| a.0 == b.0);
    }

    let mut file = match std::fs::File::create(&out_path) {
        Ok(f) => f,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/io-error",
                e.to_string(),
                Some(op),
            ));
        }
    };
    let bundle_h = {
        let mut hw = HashingWriter::new(&mut file);
        let refs_opt = if bundle_version == 2 {
            Some(refs_section.as_slice())
        } else {
            None
        };
        if let Err(e) = gc_vcs::write_bundle(&mut hw, bundle_version, root_b, &entries, refs_opt) {
            return Ok(mk_error(
                error_tok,
                "core/gpk/write-error",
                e.to_string(),
                Some(op),
            ));
        }
        hw.finish_hex()
    };
    if let Err(e) = file.sync_all() {
        return Ok(mk_error(
            error_tok,
            "core/gpk/io-error",
            e.to_string(),
            Some(op),
        ));
    }
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":ok".to_string())),
        Term::Bool(true),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":bundle-h".to_string())),
        Term::Str(bundle_h),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":bundle-v".to_string())),
        Term::Int((bundle_version as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":root".to_string())),
        Term::Str(resolved_root),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":count".to_string())),
        Term::Int((hashes.len() as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":include-evidence")),
        Term::Symbol(include_evidence.to_symbol().to_string()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":include-deps")),
        Term::Symbol(include_deps.to_symbol().to_string()),
    );
    if bundle_version == 2 {
        let out_refs: Vec<Term> = refs_section
            .iter()
            .map(|(n, h)| {
                Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":name")), Term::Str(n.clone())),
                        (TermOrdKey(Term::symbol(":hash")), Term::Str(h.clone())),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect();
        m.insert(TermOrdKey(Term::symbol(":refs")), Term::Vector(out_refs));
    }
    Ok(Value::Data(Term::Map(m)))
}

pub(super) fn handle_gpk_import(
    payload: &Term,
    ctx: &mut GpkDispatchCtx<'_>,
) -> Result<Value, EffectsError> {
    let pol = ctx.pol;
    let policy = ctx.policy;
    let store = ctx.store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/gpk-low::import".to_string())
    })?;
    let refs = ctx.refs;
    let budget = &mut *ctx.budget;
    let error_tok = ctx.error_tok;
    let op = ctx.op;
    let set_refs = match payload_gpk_set_refs(payload) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/bad-payload",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    let in_path_s = match payload_gpk_in(payload) {
        Ok(s) => s,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/bad-payload",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    let refs_db = if set_refs.is_empty() {
        None
    } else {
        Some(refs.ok_or_else(|| {
            EffectsError::Log("missing refs db for core/gpk-low::import".to_string())
        })?)
    };
    let base_dir = effective_base_dir(pol)?;
    let in_path = sandbox_path_read(&base_dir, &in_path_s)?;
    let mut f = match std::fs::File::open(&in_path) {
        Ok(f) => f,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/gpk/io-error",
                e.to_string(),
                Some(op),
            ));
        }
    };
    let max_entries = match op_extra_positive_usize(pol, "max_bundle_entries") {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let max_entry_bytes = match op_extra_positive_usize(pol, "max_bundle_entry_bytes") {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let max_bundle_bytes = match op_extra_positive_usize(pol, "max_bundle_bytes") {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let max_refs = match op_extra_positive_usize(pol, "max_bundle_refs") {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let mut limits = gc_vcs::GpkReadLimits::default_hard();
    if let Some(v) = max_entries {
        limits.max_entries = (v as u64).min(limits.max_entries);
    }
    if let Some(v) = max_entry_bytes {
        limits.max_entry_bytes = (v as u64).min(limits.max_entry_bytes);
    }
    if let Some(v) = max_bundle_bytes {
        limits.max_total_bytes = (v as u64).min(limits.max_total_bytes);
    }
    if let Some(v) = max_refs {
        limits.max_refs = (v as u64).min(limits.max_refs);
    }

    let bundle = match gc_vcs::read_bundle_with_limits(&mut f, &limits) {
        Ok(b) => b,
        Err(e) => {
            if matches!(e, gc_vcs::GpkError::LimitExceeded(_)) {
                return Ok(mk_error(
                    error_tok,
                    "core/caps/resource-limit",
                    e.to_string(),
                    Some(op),
                ));
            }
            return Ok(mk_error(
                error_tok,
                "core/gpk/read-error",
                e.to_string(),
                Some(op),
            ));
        }
    };
    let root_hex = gc_vcs::bytes32_to_hex(&bundle.root);

    for e in &bundle.entries {
        let expected = gc_vcs::bytes32_to_hex(&e.hash);
        let got = match store_put_with_budget(store, &e.bytes, policy, budget, error_tok, op) {
            Ok(h) => h,
            Err(v) => return Ok(v),
        };
        if got != expected {
            return Ok(mk_error(
                error_tok,
                "core/gpk/hash-mismatch",
                "bundle entry hash mismatch".to_string(),
                Some(op),
            ));
        }
    }

    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":ok".to_string())),
        Term::Bool(true),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":root".to_string())),
        Term::Str(root_hex),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":bundle-v".to_string())),
        Term::Int((bundle.version as i64).into()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":count".to_string())),
        Term::Int((bundle.entries.len() as i64).into()),
    );
    if !bundle.refs.is_empty() {
        let mut rs: Vec<Term> = Vec::new();
        for rr in &bundle.refs {
            rs.push(Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":name")),
                        Term::Str(rr.name.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":hash")),
                        Term::Str(gc_vcs::bytes32_to_hex(&rr.hash)),
                    ),
                ]
                .into_iter()
                .collect(),
            ));
        }
        m.insert(TermOrdKey(Term::symbol(":refs")), Term::Vector(rs));
    }
    if let Some(refs_db) = refs_db {
        let mut sorted = set_refs;
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        let mut ops: Vec<SetInput> = Vec::with_capacity(sorted.len());
        for sr in &sorted {
            if let Err(v) = local_refs_validate_policy_gate(
                store,
                &sr.name,
                sr.hash.as_deref(),
                &sr.policy,
                error_tok,
                op,
            ) {
                return Ok(v);
            }
            ops.push(SetInput {
                name: sr.name.clone(),
                new_hash: sr.hash.clone(),
                expected_old: sr.expected_old.clone(),
            });
        }
        match refs_db.set_many(&ops)? {
            SetManyResult::Updated => {
                m.insert(
                    TermOrdKey(Term::symbol(":refs-updated")),
                    Term::Int((ops.len() as i64).into()),
                );
            }
            SetManyResult::Conflict { name, current } => {
                return Ok(mk_error_with_ctx(
                    error_tok,
                    "core/refs/conflict",
                    "ref update conflict".to_string(),
                    Some(op),
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::Symbol(":refs/name".to_string())),
                                Term::Str(name),
                            ),
                            (
                                TermOrdKey(Term::Symbol(":refs/current".to_string())),
                                current.map(Term::Str).unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                ));
            }
        }
    }
    Ok(Value::Data(Term::Map(m)))
}
