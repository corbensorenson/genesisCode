use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "capability dispatch signatures are explicit by design"
)]
pub(super) fn dispatch_meta(
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
    let _ = (pol, policy, budget, timeout_ms);
    match op_eff {
        "core/vcs-low::log" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::log".to_string())
            })?;

            let root_s = match payload_vcs_root(payload) {
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
            let max = payload_vcs_max(payload).unwrap_or(1000);

            let mut root_commit = root_s.clone();
            if root_commit.starts_with("refs/") {
                let Some(rdb) = refs else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/missing-refs-db",
                        "root is a ref name but refs db is not configured".to_string(),
                        Some(op),
                    ));
                };
                let cur = match rdb.get(&root_commit) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/refs-io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let Some(h) = cur else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/ref-not-found",
                        format!("ref not found: {root_commit}"),
                        Some(op),
                    ));
                };
                root_commit = h;
            }

            if gc_vcs::validate_hex_hash(&root_commit).is_err() {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-root",
                    "root must be a 64-hex commit hash or refs/...".to_string(),
                    Some(op),
                ));
            }

            use std::collections::HashSet;
            let mut visited: HashSet<String> = HashSet::new();
            let mut out: Vec<Term> = Vec::new();
            let mut stack: Vec<String> = vec![root_commit.clone()];

            let mut truncated = false;
            while let Some(h) = stack.pop() {
                if out.len() as u64 >= max {
                    truncated = true;
                    break;
                }
                if !visited.insert(h.clone()) {
                    continue;
                }
                let p = store.path_for(&h);
                if !p.exists() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {h}"),
                        Some(op),
                    ));
                }
                let t = match store_get_term(store, &h) {
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
                let c = match gc_vcs::Commit::from_term(&t) {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/bad-commit",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };

                // Deterministic parent traversal: preserve stored order.
                for parent in c.parents.iter().rev() {
                    stack.push(parent.clone());
                }

                let mut cm = BTreeMap::new();
                cm.insert(TermOrdKey(Term::symbol(":hash")), Term::Str(h));
                cm.insert(
                    TermOrdKey(Term::symbol(":parents")),
                    Term::Vector(c.parents.iter().cloned().map(Term::Str).collect()),
                );
                cm.insert(
                    TermOrdKey(Term::symbol(":base")),
                    match c.base {
                        Some(b) => Term::Str(b),
                        None => Term::Nil,
                    },
                );
                cm.insert(TermOrdKey(Term::symbol(":patch")), Term::Str(c.patch));
                cm.insert(TermOrdKey(Term::symbol(":result")), Term::Str(c.result));
                cm.insert(
                    TermOrdKey(Term::symbol(":obligations")),
                    Term::Vector(c.obligations.iter().cloned().map(Term::Str).collect()),
                );
                cm.insert(
                    TermOrdKey(Term::symbol(":evidence")),
                    Term::Vector(c.evidence.iter().cloned().map(Term::Str).collect()),
                );
                cm.insert(
                    TermOrdKey(Term::symbol(":attestations")),
                    Term::Vector(c.attestations.iter().cloned().map(Term::Str).collect()),
                );
                cm.insert(TermOrdKey(Term::symbol(":message")), Term::Str(c.message));
                out.push(Term::Map(cm));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":root")), Term::Str(root_commit));
            m.insert(
                TermOrdKey(Term::symbol(":truncated")),
                Term::Bool(truncated),
            );
            m.insert(TermOrdKey(Term::symbol(":commits")), Term::Vector(out));
            Ok(Value::data(Term::Map(m)))
        }
        "core/vcs-low::blame" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::blame".to_string())
            })?;

            let sym = match payload_vcs_sym(payload, ":sym") {
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
            let path = match payload_vcs_opt_sym_or_str(payload, ":path") {
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
            let snapshot_h = match payload_vcs_opt_hash(payload, ":snapshot") {
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
            let commit_h = match payload_vcs_opt_hash(payload, ":commit") {
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

            if snapshot_h.is_none() && commit_h.is_none() {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :snapshot or :commit".to_string(),
                    Some(op),
                ));
            }

            let start_commit = if let Some(ch) = commit_h {
                ch
            } else {
                let Some(rdb) = refs else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/missing-refs-db",
                        "snapshot lookup requires refs db".to_string(),
                        Some(op),
                    ));
                };
                let Some(sh) = snapshot_h.clone() else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        "missing :snapshot".to_string(),
                        Some(op),
                    ));
                };
                let found = match vcs_find_commit_for_snapshot(store, rdb, &sh) {
                    Ok(x) => x,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op)));
                    }
                };
                let Some(h) = found else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/no-commit-for-snapshot",
                        format!("no commit found for snapshot: {sh}"),
                        Some(op),
                    ));
                };
                h
            };

            let (start_commit_obj, _) = match vcs_load_commit(store, &start_commit) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/bad-commit", e, Some(op))),
            };

            if let Some(sh) = &snapshot_h
                && &start_commit_obj.result != sh
            {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "provided :commit does not resolve to provided :snapshot".to_string(),
                    Some(op),
                ));
            }

            let query_snapshot = snapshot_h.unwrap_or(start_commit_obj.result.clone());
            let value_h = match vcs_snapshot_symbol_ref(store, &query_snapshot, &sym) {
                Ok(Some(h)) => h,
                Ok(None) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/symbol-not-found",
                        format!("symbol not found in snapshot: {sym}"),
                        Some(op),
                    ));
                }
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op))),
            };

            let blame_h = match vcs_blame_commit_for_symbol(store, &start_commit, &sym, &value_h) {
                Ok(h) => h,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op))),
            };
            let (blame_commit, _) = match vcs_load_commit(store, &blame_h) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/bad-commit", e, Some(op))),
            };

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym));
            m.insert(TermOrdKey(Term::symbol(":value")), Term::Str(value_h));
            m.insert(TermOrdKey(Term::symbol(":commit")), Term::Str(blame_h));
            m.insert(
                TermOrdKey(Term::symbol(":snapshot")),
                Term::Str(blame_commit.result),
            );
            m.insert(
                TermOrdKey(Term::symbol(":query-snapshot")),
                Term::Str(query_snapshot),
            );
            m.insert(
                TermOrdKey(Term::symbol(":path")),
                path.map(Term::Str).unwrap_or(Term::Nil),
            );
            Ok(Value::data(Term::Map(m)))
        }
        "core/vcs-low::why" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::why".to_string())
            })?;

            let sym = match payload_vcs_sym(payload, ":sym") {
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
            let op_sym = match payload_vcs_opt_sym_or_str(payload, ":op") {
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
            let path = match payload_vcs_opt_sym_or_str(payload, ":path") {
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
            let snapshot_h = match payload_vcs_opt_hash(payload, ":snapshot") {
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
            let commit_h = match payload_vcs_opt_hash(payload, ":commit") {
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

            if snapshot_h.is_none() && commit_h.is_none() {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :snapshot or :commit".to_string(),
                    Some(op),
                ));
            }

            let start_commit = if let Some(ch) = commit_h {
                ch
            } else {
                let Some(rdb) = refs else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/missing-refs-db",
                        "snapshot lookup requires refs db".to_string(),
                        Some(op),
                    ));
                };
                let Some(sh) = snapshot_h.clone() else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        "missing :snapshot".to_string(),
                        Some(op),
                    ));
                };
                let found = match vcs_find_commit_for_snapshot(store, rdb, &sh) {
                    Ok(x) => x,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op)));
                    }
                };
                let Some(h) = found else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/no-commit-for-snapshot",
                        format!("no commit found for snapshot: {sh}"),
                        Some(op),
                    ));
                };
                h
            };

            let (start_commit_obj, _) = match vcs_load_commit(store, &start_commit) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/bad-commit", e, Some(op))),
            };
            if let Some(sh) = &snapshot_h
                && &start_commit_obj.result != sh
            {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "provided :commit does not resolve to provided :snapshot".to_string(),
                    Some(op),
                ));
            }

            let query_snapshot = snapshot_h.unwrap_or(start_commit_obj.result.clone());
            let value_h = match vcs_snapshot_symbol_ref(store, &query_snapshot, &sym) {
                Ok(Some(h)) => h,
                Ok(None) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/symbol-not-found",
                        format!("symbol not found in snapshot: {sym}"),
                        Some(op),
                    ));
                }
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op))),
            };
            let blame_h = match vcs_blame_commit_for_symbol(store, &start_commit, &sym, &value_h) {
                Ok(h) => h,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op))),
            };

            let (blame_commit, blame_term) = match vcs_load_commit(store, &blame_h) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/bad-commit", e, Some(op))),
            };
            let (target, author, why) = match &blame_term {
                Term::Map(mm) => (
                    mm.get(&TermOrdKey(Term::symbol(":target")))
                        .cloned()
                        .unwrap_or(Term::Nil),
                    mm.get(&TermOrdKey(Term::symbol(":author")))
                        .cloned()
                        .unwrap_or(Term::Nil),
                    mm.get(&TermOrdKey(Term::symbol(":why")))
                        .cloned()
                        .unwrap_or(Term::Nil),
                ),
                _ => (Term::Nil, Term::Nil, Term::Nil),
            };

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym));
            m.insert(TermOrdKey(Term::symbol(":value")), Term::Str(value_h));
            m.insert(TermOrdKey(Term::symbol(":commit")), Term::Str(blame_h));
            m.insert(
                TermOrdKey(Term::symbol(":snapshot")),
                Term::Str(blame_commit.result),
            );
            m.insert(
                TermOrdKey(Term::symbol(":query-snapshot")),
                Term::Str(query_snapshot),
            );
            m.insert(
                TermOrdKey(Term::symbol(":message")),
                Term::Str(blame_commit.message),
            );
            m.insert(
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(
                    blame_commit
                        .obligations
                        .into_iter()
                        .map(Term::Str)
                        .collect(),
                ),
            );
            m.insert(
                TermOrdKey(Term::symbol(":evidence")),
                Term::Vector(blame_commit.evidence.into_iter().map(Term::Str).collect()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":attestations")),
                Term::Vector(
                    blame_commit
                        .attestations
                        .into_iter()
                        .map(Term::Str)
                        .collect(),
                ),
            );
            m.insert(
                TermOrdKey(Term::symbol(":path")),
                path.map(Term::Str).unwrap_or(Term::Nil),
            );
            m.insert(
                TermOrdKey(Term::symbol(":op")),
                op_sym.map(Term::Str).unwrap_or(Term::Nil),
            );
            m.insert(TermOrdKey(Term::symbol(":target")), target);
            m.insert(TermOrdKey(Term::symbol(":author")), author);
            m.insert(TermOrdKey(Term::symbol(":why")), why);
            Ok(Value::data(Term::Map(m)))
        }
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op-eff",
            format!("core/vcs-low dispatch received unsupported op_eff: {op_eff}"),
            Some(op),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_vcs_low_op_eff_returns_sealed_error_instead_of_panicking() {
        let mut budget = ArtifactBudgetState::default();
        let out = dispatch_meta(
            "core/vcs-low::unsupported-op",
            &Term::Nil,
            None,
            &CapsPolicy::empty(),
            None,
            None,
            &mut budget,
            SealId(991),
            "core/vcs-low::log",
            None,
        )
        .expect("dispatch should return value");

        match out {
            Value::Sealed { token, payload } => {
                assert_eq!(token, SealId(991));
                let Some(Term::Map(mm)) = payload.as_ref().as_data() else {
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
