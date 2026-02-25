use super::*;

pub(super) fn handle_resolve_conflict(
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
) -> Result<Value, EffectsError> {
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/vcs-low::resolve-conflict".to_string())
    })?;
    let Term::Map(m) = payload else {
        return Ok(mk_error(
            error_tok,
            "core/vcs/bad-payload",
            "payload must be a map".to_string(),
            Some(op),
        ));
    };
    let out_s = match m.get(&TermOrdKey(Term::symbol(":out"))) {
        None | Some(Term::Nil) => None,
        Some(Term::Str(s)) => Some(s.clone()),
        Some(other) => {
            return Ok(mk_error(
                error_tok,
                "core/vcs/bad-payload",
                format!(":out must be string or nil, got {}", print_term(other)),
                Some(op),
            ));
        }
    };
    let (conflict, base_t, left_t, right_t, legacy_output_mode) = if let (
        Some(conflict_t),
        Some(base_t),
        Some(left_t),
        Some(right_t),
    ) = (
        m.get(&TermOrdKey(Term::symbol(":conflict-term"))),
        m.get(&TermOrdKey(Term::symbol(":base-term"))),
        m.get(&TermOrdKey(Term::symbol(":left-term"))),
        m.get(&TermOrdKey(Term::symbol(":right-term"))),
    ) {
        let conflict = match gc_vcs::Conflict::from_term(conflict_t) {
            Ok(c) => c,
            Err(e) => {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-conflict",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        (
            conflict,
            base_t.clone(),
            left_t.clone(),
            right_t.clone(),
            false,
        )
    } else if let Some(Term::Str(conflict_h)) = m.get(&TermOrdKey(Term::symbol(":conflict-hash"))) {
        let conflict_t = match store_get_term(store, conflict_h) {
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
        let conflict = match gc_vcs::Conflict::from_term(&conflict_t) {
            Ok(c) => c,
            Err(e) => {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-conflict",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        let base_t = match store_get_term(store, &conflict.base) {
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
        let left_t = match store_get_term(store, &conflict.left) {
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
        let right_t = match store_get_term(store, &conflict.right) {
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
        (conflict, base_t, left_t, right_t, false)
    } else if let Some(Term::Str(conflict_h)) = m.get(&TermOrdKey(Term::symbol(":conflict"))) {
        let conflict_t = match store_get_term(store, conflict_h) {
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
        let conflict = match gc_vcs::Conflict::from_term(&conflict_t) {
            Ok(c) => c,
            Err(e) => {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-conflict",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        let base_t = match store_get_term(store, &conflict.base) {
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
        let left_t = match store_get_term(store, &conflict.left) {
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
        let right_t = match store_get_term(store, &conflict.right) {
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
        (conflict, base_t, left_t, right_t, true)
    } else {
        return Ok(mk_error(
            error_tok,
            "core/vcs/bad-payload",
            "missing :conflict/:conflict-hash or (:conflict-term + :base-term/:left-term/:right-term)"
                .to_string(),
            Some(op),
        ));
    };

    let strategy = match m.get(&TermOrdKey(Term::symbol(":strategy"))) {
        None | Some(Term::Nil) => None,
        Some(Term::Symbol(s)) => Some(s.clone()),
        Some(Term::Str(s)) => Some(s.clone()),
        Some(other) => {
            return Ok(mk_error(
                error_tok,
                "core/vcs/bad-payload",
                format!(
                    ":strategy must be symbol/string or nil, got {}",
                    print_term(other)
                ),
                Some(op),
            ));
        }
    };
    let strategy = strategy.map(|s| match s.as_str() {
        ":left" | "left" => ":left".to_string(),
        ":right" | "right" => ":right".to_string(),
        ":base" | "base" => ":base".to_string(),
        other => other.to_string(),
    });
    if let Some(s) = &strategy
        && s != ":left"
        && s != ":right"
        && s != ":base"
    {
        return Ok(mk_error(
            error_tok,
            "core/vcs/bad-payload",
            format!("unsupported :strategy {s} (expected :left/:right/:base)"),
            Some(op),
        ));
    }

    #[derive(Debug, Clone)]
    enum Resolution {
        Side(String),
        Hash(String),
        Delete,
    }
    let mut resolutions: BTreeMap<String, Resolution> = BTreeMap::new();
    if let Some(t) = m.get(&TermOrdKey(Term::symbol(":resolutions"))) {
        let Term::Map(rm) = t else {
            return Ok(mk_error(
                error_tok,
                "core/vcs/bad-payload",
                format!(":resolutions must be map, got {}", print_term(t)),
                Some(op),
            ));
        };
        for (k, v) in rm {
            let opk = match &k.0 {
                Term::Symbol(s) => s.clone(),
                Term::Str(s) => s.clone(),
                other => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!(
                            ":resolutions keys must be symbol/string, got {}",
                            print_term(other)
                        ),
                        Some(op),
                    ));
                }
            };
            let res = match v {
                Term::Nil => Resolution::Delete,
                Term::Symbol(s) => match s.as_str() {
                    ":left" | "left" => Resolution::Side(":left".to_string()),
                    ":right" | "right" => Resolution::Side(":right".to_string()),
                    ":base" | "base" => Resolution::Side(":base".to_string()),
                    other => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/bad-payload",
                            format!(
                                ":resolutions/{opk} unsupported side {other} (expected :left/:right/:base)"
                            ),
                            Some(op),
                        ));
                    }
                },
                Term::Str(s) => match s.as_str() {
                    ":left" | "left" => Resolution::Side(":left".to_string()),
                    ":right" | "right" => Resolution::Side(":right".to_string()),
                    ":base" | "base" => Resolution::Side(":base".to_string()),
                    _ => {
                        if let Err(e) = gc_vcs::validate_hex_hash(s) {
                            return Ok(mk_error(
                                error_tok,
                                "core/vcs/bad-payload",
                                format!(":resolutions/{opk}: {e}"),
                                Some(op),
                            ));
                        }
                        Resolution::Hash(s.clone())
                    }
                },
                other => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!(
                            ":resolutions/{opk} must be side symbol, hex string, or nil; got {}",
                            print_term(other)
                        ),
                        Some(op),
                    ));
                }
            };
            resolutions.insert(opk, res);
        }
    }

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
    if base.proto != left.proto || base.proto != right.proto {
        return Ok(mk_error(
            error_tok,
            "core/vcs/bad-conflict",
            "proto mismatch across base/left/right".to_string(),
            Some(op),
        ));
    }

    let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    keys.extend(base.overrides.keys().cloned());
    keys.extend(left.overrides.keys().cloned());
    keys.extend(right.overrides.keys().cloned());

    let mut merged: BTreeMap<String, String> = BTreeMap::new();
    let mut unresolved: Vec<Term> = Vec::new();

    for k in keys {
        let b = base.overrides.get(&k).cloned();
        let l = left.overrides.get(&k).cloned();
        let r = right.overrides.get(&k).cloned();

        let conflict_here = l != r && l != b && r != b;
        if !conflict_here {
            let pick = if l == r {
                l
            } else if l == b {
                r
            } else if r == b {
                l
            } else {
                None
            };
            if let Some(h) = pick {
                merged.insert(k, h);
            }
            continue;
        }

        let chosen = resolutions
            .get(&k)
            .cloned()
            .or_else(|| strategy.as_ref().map(|s| Resolution::Side(s.clone())));

        let picked = match chosen {
            Some(Resolution::Side(s)) if s == ":left" => l,
            Some(Resolution::Side(s)) if s == ":right" => r,
            Some(Resolution::Side(s)) if s == ":base" => b,
            Some(Resolution::Hash(h)) => Some(h),
            Some(Resolution::Delete) => None,
            _ => {
                let mut mm = BTreeMap::new();
                mm.insert(TermOrdKey(Term::symbol(":op")), Term::Str(k.clone()));
                mm.insert(
                    TermOrdKey(Term::symbol(":base")),
                    b.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":left")),
                    l.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":right")),
                    r.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                unresolved.push(Term::Map(mm));
                continue;
            }
        };

        if let Some(h) = picked {
            if !store.path_for(&h).exists() {
                return Ok(mk_error(
                    error_tok,
                    "core/store/not-found",
                    format!("missing referenced artifact: {h}"),
                    Some(op),
                ));
            }
            merged.insert(k, h);
        }
    }

    if !unresolved.is_empty() {
        let conflict_term = mk_conflict_artifact(
            &conflict.kind,
            &conflict.base,
            &conflict.left,
            &conflict.right,
            unresolved,
        );
        if legacy_output_mode {
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
                if let Err(e) = atomic_write_text(&out_path, (conflict_bytes + "\n").as_bytes()) {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            }
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
            out.insert(TermOrdKey(Term::symbol(":conflict")), Term::Str(conflict_h));
            return Ok(Value::Data(Term::Map(out)));
        }
        let mut out = BTreeMap::new();
        out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
        out.insert(TermOrdKey(Term::symbol(":conflict-term")), conflict_term);
        return Ok(Value::Data(Term::Map(out)));
    }

    let merged_snapshot = gc_vcs::ContractSnapshot {
        proto: base.proto,
        overrides: merged,
    }
    .to_term();
    let (patch_term, values) = match vcs_diff_patch_term(store, &base_t, &merged_snapshot) {
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
    if legacy_output_mode {
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
        let patch_bytes = print_term(&patch_term);
        let patch_h = match store_put_with_budget(
            store,
            patch_bytes.as_bytes(),
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
        let mut out = BTreeMap::new();
        out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
        out.insert(TermOrdKey(Term::symbol(":snapshot")), Term::Str(merged_h));
        out.insert(TermOrdKey(Term::symbol(":patch")), Term::Str(patch_h));
        out.insert(
            TermOrdKey(Term::symbol(":values")),
            Term::Vector(values.into_iter().map(Term::Str).collect()),
        );
        return Ok(Value::Data(Term::Map(out)));
    }
    let mut out = BTreeMap::new();
    out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    out.insert(TermOrdKey(Term::symbol(":snapshot-term")), merged_snapshot);
    out.insert(TermOrdKey(Term::symbol(":patch-term")), patch_term);
    out.insert(
        TermOrdKey(Term::symbol(":values")),
        Term::Vector(values.into_iter().map(Term::Str).collect()),
    );
    Ok(Value::Data(Term::Map(out)))
}
