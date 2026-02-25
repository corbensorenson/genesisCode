use super::*;

pub(super) fn handle_merge3_contract_snapshots(
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Term::Map(m) = payload else {
        return mk_error(
            error_tok,
            "core/vcs/bad-payload",
            "payload must be a map".to_string(),
            Some(op),
        );
    };
    let Some(base_t) = m.get(&TermOrdKey(Term::symbol(":base-term"))) else {
        return mk_error(
            error_tok,
            "core/vcs/bad-payload",
            "missing :base-term".to_string(),
            Some(op),
        );
    };
    let Some(left_t) = m.get(&TermOrdKey(Term::symbol(":left-term"))) else {
        return mk_error(
            error_tok,
            "core/vcs/bad-payload",
            "missing :left-term".to_string(),
            Some(op),
        );
    };
    let Some(right_t) = m.get(&TermOrdKey(Term::symbol(":right-term"))) else {
        return mk_error(
            error_tok,
            "core/vcs/bad-payload",
            "missing :right-term".to_string(),
            Some(op),
        );
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
            return mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op));
        }
    };
    let left = match as_contract_snapshot(left_t) {
        Ok(s) => s,
        Err(msg) => {
            return mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op));
        }
    };
    let right = match as_contract_snapshot(right_t) {
        Ok(s) => s,
        Err(msg) => {
            return mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op));
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
        return Value::Data(Term::Map(out));
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
        return Value::Data(Term::Map(out));
    }

    let merged_snapshot = gc_vcs::ContractSnapshot {
        proto: base.proto,
        overrides: merged,
    }
    .to_term();
    let mut out = BTreeMap::new();
    out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    out.insert(TermOrdKey(Term::symbol(":snapshot-term")), merged_snapshot);
    Value::Data(Term::Map(out))
}
