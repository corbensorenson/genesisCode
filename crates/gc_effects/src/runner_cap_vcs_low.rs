use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "host capability dispatch wiring keeps explicit context parameters visible"
)]
pub(super) fn capability_vcs_low(
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
    let _ = (policy, timeout_ms);
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
            Ok(Value::Data(Term::Map(m)))
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
            Ok(Value::Data(Term::Map(m)))
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
            Ok(Value::Data(Term::Map(m)))
        }
        "core/vcs-low::diff-terms" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::diff-terms".to_string())
            })?;
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let Some(base_t) = m.get(&TermOrdKey(Term::symbol(":base-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :base-term".to_string(),
                    Some(op),
                ));
            };
            let Some(to_t) = m.get(&TermOrdKey(Term::symbol(":to-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :to-term".to_string(),
                    Some(op),
                ));
            };
            let (patch_term, values) = match vcs_diff_patch_term(store, base_t, to_t) {
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
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":patch-term")), patch_term);
            out.insert(
                TermOrdKey(Term::symbol(":values")),
                Term::Vector(values.into_iter().map(Term::Str).collect()),
            );
            Ok(Value::Data(Term::Map(out)))
        }
        "core/vcs-low::apply-patch" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log(
                    "missing artifact store for core/vcs-low::apply-patch".to_string(),
                )
            })?;
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let Some(base_t) = m.get(&TermOrdKey(Term::symbol(":base-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :base-term".to_string(),
                    Some(op),
                ));
            };
            let Some(patch_t) = m.get(&TermOrdKey(Term::symbol(":patch-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :patch-term".to_string(),
                    Some(op),
                ));
            };
            let patch = match gc_vcs::Patch::from_term(patch_t) {
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
            let snapshot_t = match vcs_apply_patch_term(store, base_t, &patch) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/vcs/apply-error", e, Some(op)));
                }
            };
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":snapshot-term")), snapshot_t);
            Ok(Value::Data(Term::Map(out)))
        }
        "core/vcs-low::merge3-contract-snapshots" => {
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let Some(base_t) = m.get(&TermOrdKey(Term::symbol(":base-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :base-term".to_string(),
                    Some(op),
                ));
            };
            let Some(left_t) = m.get(&TermOrdKey(Term::symbol(":left-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :left-term".to_string(),
                    Some(op),
                ));
            };
            let Some(right_t) = m.get(&TermOrdKey(Term::symbol(":right-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :right-term".to_string(),
                    Some(op),
                ));
            };
            let term_hash = |t: &Term| hash_bytes_hex(print_term(t).as_bytes());
            let base_h = match m.get(&TermOrdKey(Term::symbol(":base-hash"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => term_hash(base_t),
            };
            let left_h = match m.get(&TermOrdKey(Term::symbol(":left-hash"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => term_hash(left_t),
            };
            let right_h = match m.get(&TermOrdKey(Term::symbol(":right-hash"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => term_hash(right_t),
            };

            let base = match as_contract_snapshot(base_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            let left = match as_contract_snapshot(left_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            let right = match as_contract_snapshot(right_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };

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
                let mut out = BTreeMap::new();
                out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
                out.insert(TermOrdKey(Term::symbol(":conflict-term")), conflict_term);
                return Ok(Value::Data(Term::Map(out)));
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
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":snapshot-term")), merged_snapshot);
            Ok(Value::Data(Term::Map(out)))
        }
        "core/vcs-low::resolve-conflict" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log(
                    "missing artifact store for core/vcs-low::resolve-conflict".to_string(),
                )
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
            } else if let Some(Term::Str(conflict_h)) =
                m.get(&TermOrdKey(Term::symbol(":conflict-hash")))
            {
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
            } else if let Some(Term::Str(conflict_h)) =
                m.get(&TermOrdKey(Term::symbol(":conflict")))
            {
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
                        if let Err(e) =
                            atomic_write_text(&out_path, (conflict_bytes + "\n").as_bytes())
                        {
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
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op",
            format!("unknown capability op: {op}"),
            Some(op),
        )),
    }
}
