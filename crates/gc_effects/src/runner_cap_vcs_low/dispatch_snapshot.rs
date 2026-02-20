use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "capability dispatch signatures are explicit by design"
)]
pub(super) fn dispatch_snapshot(
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
    let _ = (policy, refs, timeout_ms);
    match op_eff {
        "core/vcs-low::diff" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::diff".to_string())
            })?;

            let base_h = match payload_vcs_hash(payload, ":base") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let to_h = match payload_vcs_hash(payload, ":to") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let out_s = match payload_vcs_out(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let store_patch = payload_vcs_store(payload).unwrap_or(true);

            let base_t = match store_get_term(store, &base_h) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/store-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let to_t = match store_get_term(store, &to_h) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/store-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            let (patch_term, values) = match vcs_diff_patch_term(store, &base_t, &to_t) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/diff-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let patch_bytes = print_term(&patch_term);
            let patch_h = if store_patch {
                match store_put_with_budget(
                    store,
                    patch_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                }
            } else {
                hash_bytes_hex(patch_bytes.as_bytes())
            };

            if let Some(out_s) = out_s {
                let base_dir = effective_base_dir(pol)?;
                let out_path = sandbox_path_write(
                    &base_dir,
                    &out_s,
                    pol.map(|p| p.create_dirs).unwrap_or(false),
                )?;
                if let Err(e) =
                    atomic_write_text(&out_path, (patch_bytes.clone() + "\n").as_bytes())
                {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":patch")), Term::Str(patch_h));
            m.insert(
                TermOrdKey(Term::symbol(":values")),
                Term::Vector(values.into_iter().map(Term::Str).collect()),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/vcs-low::apply" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::apply".to_string())
            })?;

            let base_h = match payload_vcs_hash(payload, ":base") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let patch_s = match payload_vcs_patch(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let out_s = match payload_vcs_out(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let store_result = payload_vcs_store(payload).unwrap_or(true);

            let base_t = match store_get_term(store, &base_h) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/store-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            let base_dir = effective_base_dir(pol)?;
            let patch_term = if gc_vcs::validate_hex_hash(&patch_s).is_ok() {
                match store_get_term(store, &patch_s) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-found",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                }
            } else {
                let patch_path = match sandbox_path_read(&base_dir, &patch_s) {
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
                let s = match std::fs::read_to_string(&patch_path) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                match gc_coreform::parse_term(&s) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/parse-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                }
            };

            let patch = match gc_vcs::Patch::from_term(&patch_term) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-patch",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let cur = match vcs_apply_patch_term(store, &base_t, &patch) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/vcs/apply-error", e, Some(op)));
                }
            };

            let snap_bytes = print_term(&cur);
            let snap_h = if store_result {
                match store_put_with_budget(
                    store,
                    snap_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                }
            } else {
                hash_bytes_hex(snap_bytes.as_bytes())
            };

            if let Some(out_s) = out_s {
                let out_path = sandbox_path_write(
                    &base_dir,
                    &out_s,
                    pol.map(|p| p.create_dirs).unwrap_or(false),
                )?;
                if let Err(e) = atomic_write_text(&out_path, (snap_bytes.clone() + "\n").as_bytes())
                {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":snapshot")), Term::Str(snap_h));
            Ok(Value::Data(Term::Map(m)))
        }
        "core/vcs-low::merge3" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::merge3".to_string())
            })?;

            let out_s = match payload_vcs_out(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let base_h = match payload_vcs_hash(payload, ":base") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let left_h = match payload_vcs_hash(payload, ":left") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let right_h = match payload_vcs_hash(payload, ":right") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let base_t = store_get_term(store, &base_h)
                .map_err(|e| EffectsError::Log(format!("merge3 base read error: {e}")))?;
            let left_t = store_get_term(store, &left_h)
                .map_err(|e| EffectsError::Log(format!("merge3 left read error: {e}")))?;
            let right_t = store_get_term(store, &right_h)
                .map_err(|e| EffectsError::Log(format!("merge3 right read error: {e}")))?;

            let base = match as_contract_snapshot(&base_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            let left = match as_contract_snapshot(&left_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            let right = match as_contract_snapshot(&right_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };

            // Proto must be stable across all three snapshots for contract merge.
            if base.proto != left.proto || base.proto != right.proto {
                let conflict_term = mk_conflict_artifact(
                    ":contract-snapshot-merge3",
                    &base_h,
                    &left_h,
                    &right_h,
                    vec![Term::Map(
                        [
                            (TermOrdKey(Term::symbol(":op")), Term::symbol(":proto")),
                            (
                                TermOrdKey(Term::symbol(":base")),
                                base.proto.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                            (
                                TermOrdKey(Term::symbol(":left")),
                                left.proto.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                            (
                                TermOrdKey(Term::symbol(":right")),
                                right.proto.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    )],
                );
                let conflict_bytes = print_term(&conflict_term);
                let conflict_h = match store_put_with_budget(
                    store,
                    conflict_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                };

                if let Some(out_s) = &out_s {
                    let base_dir = effective_base_dir(pol)?;
                    let out_path = sandbox_path_write(
                        &base_dir,
                        out_s,
                        pol.map(|p| p.create_dirs).unwrap_or(false),
                    )?;
                    if let Err(e) = atomic_write_text(&out_path, (conflict_bytes + "\n").as_bytes())
                    {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                }

                let mut m = BTreeMap::new();
                m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
                m.insert(TermOrdKey(Term::symbol(":conflict")), Term::Str(conflict_h));
                return Ok(Value::Data(Term::Map(m)));
            }

            let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            keys.extend(base.overrides.keys().cloned());
            keys.extend(left.overrides.keys().cloned());
            keys.extend(right.overrides.keys().cloned());

            let mut merged: BTreeMap<String, String> = BTreeMap::new();
            let mut conflicts: Vec<Term> = Vec::new();

            for k in keys {
                let b = base.overrides.get(&k).cloned();
                let l = left.overrides.get(&k).cloned();
                let r = right.overrides.get(&k).cloned();

                let pick = if l == r {
                    l.clone()
                } else if l == b {
                    r.clone()
                } else if r == b {
                    l.clone()
                } else {
                    None
                };

                if l == r || l == b || r == b {
                    if let Some(h) = pick {
                        merged.insert(k.clone(), h);
                    }
                    continue;
                }

                // One-side change from None / deletion can still be cleanly merged.
                // Treat absence as None; if one side differs from base and the other equals base, we already handled.
                // Remaining cases are conflicts.
                conflicts.push(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":op")), Term::Symbol(k.clone())),
                        (
                            TermOrdKey(Term::symbol(":base")),
                            b.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":left")),
                            l.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":right")),
                            r.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }

            if !conflicts.is_empty() {
                conflicts.sort_by_cached_key(print_term);
                let conflict_term = mk_conflict_artifact(
                    ":contract-snapshot-merge3",
                    &base_h,
                    &left_h,
                    &right_h,
                    conflicts,
                );
                let conflict_bytes = print_term(&conflict_term);
                let conflict_h = match store_put_with_budget(
                    store,
                    conflict_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                };

                if let Some(out_s) = &out_s {
                    let base_dir = effective_base_dir(pol)?;
                    let out_path = sandbox_path_write(
                        &base_dir,
                        out_s,
                        pol.map(|p| p.create_dirs).unwrap_or(false),
                    )?;
                    if let Err(e) = atomic_write_text(&out_path, (conflict_bytes + "\n").as_bytes())
                    {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                }

                let mut m = BTreeMap::new();
                m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
                m.insert(TermOrdKey(Term::symbol(":conflict")), Term::Str(conflict_h));
                return Ok(Value::Data(Term::Map(m)));
            }

            let merged_snapshot = gc_vcs::ContractSnapshot {
                proto: base.proto,
                overrides: merged,
            }
            .to_term();
            let merged_bytes = print_term(&merged_snapshot);
            let merged_h = match store_put_with_budget(
                store,
                merged_bytes.as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            if let Some(out_s) = &out_s {
                let base_dir = effective_base_dir(pol)?;
                let out_path = sandbox_path_write(
                    &base_dir,
                    out_s,
                    pol.map(|p| p.create_dirs).unwrap_or(false),
                )?;
                if let Err(e) = atomic_write_text(&out_path, (merged_bytes + "\n").as_bytes()) {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":snapshot")), Term::Str(merged_h));
            Ok(Value::Data(Term::Map(m)))
        }
        _ => unreachable!("dispatch_snapshot called with unsupported op: {op_eff}"),
    }
}
