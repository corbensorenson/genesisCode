use super::*;

pub(super) fn vcs_load_commit(
    store: &ArtifactStore,
    commit_h: &str,
) -> Result<(gc_vcs::Commit, Term), String> {
    let t = store_get_term(store, commit_h).map_err(|e| e.to_string())?;
    let c = gc_vcs::Commit::from_term(&t).map_err(|e| format!("bad commit {commit_h}: {e}"))?;
    Ok((c, t))
}

pub(super) fn vcs_snapshot_symbol_ref(
    store: &ArtifactStore,
    snapshot_h: &str,
    sym: &str,
) -> Result<Option<String>, String> {
    let t = store_get_term(store, snapshot_h).map_err(|e| e.to_string())?;
    let snap =
        gc_vcs::Snapshot::from_term(&t).map_err(|e| format!("bad snapshot {snapshot_h}: {e}"))?;
    match snap.kind {
        gc_vcs::SnapshotKind::Module(m) => Ok(m.defs.get(sym).cloned()),
        gc_vcs::SnapshotKind::Contract(c) => Ok(c.overrides.get(sym).cloned()),
        gc_vcs::SnapshotKind::Workspace(w) => Ok(w.modules.get(sym).cloned()),
        gc_vcs::SnapshotKind::Package(p) => {
            for me in p.modules {
                if me.path == sym {
                    return Ok(Some(me.hash_hex));
                }
            }
            Ok(None)
        }
    }
}

pub(super) fn vcs_find_commit_for_snapshot(
    store: &ArtifactStore,
    refs: &RefsDb,
    snapshot_h: &str,
) -> Result<Option<String>, String> {
    use std::collections::HashSet;

    let refs = refs.list(None).map_err(|e| e.to_string())?;
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = refs
        .into_iter()
        .filter_map(|r| r.hash)
        .filter(|h| gc_vcs::validate_hex_hash(h).is_ok())
        .collect();

    stack.sort();
    stack.dedup();

    while let Some(h) = stack.pop() {
        if !visited.insert(h.clone()) {
            continue;
        }
        let (c, _) = vcs_load_commit(store, &h)?;
        if c.result == snapshot_h {
            return Ok(Some(h));
        }
        for parent in c.parents.iter().rev() {
            stack.push(parent.clone());
        }
    }
    Ok(None)
}

pub(super) fn vcs_blame_commit_for_symbol(
    store: &ArtifactStore,
    start_commit_h: &str,
    sym: &str,
    value_h: &str,
) -> Result<String, String> {
    use std::collections::HashSet;

    let mut cur = start_commit_h.to_string();
    let mut seen: HashSet<String> = HashSet::new();
    loop {
        if !seen.insert(cur.clone()) {
            return Ok(cur);
        }
        let (c, _) = vcs_load_commit(store, &cur)?;
        let mut next_parent: Option<String> = None;
        for p in &c.parents {
            let (pc, _) = vcs_load_commit(store, p)?;
            let pref = vcs_snapshot_symbol_ref(store, &pc.result, sym)?;
            if pref.as_deref() == Some(value_h) {
                next_parent = Some(p.clone());
                break;
            }
        }
        match next_parent {
            Some(n) => cur = n,
            None => return Ok(cur),
        }
    }
}

// -----------------------------------------------------------------------------
// VCS merge3 (contract snapshots)
// -----------------------------------------------------------------------------

pub(super) fn hash_bytes_hex(bytes: &[u8]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(bytes);
    h.finalize().to_hex().to_string()
}

pub(super) fn as_contract_snapshot(t: &Term) -> Result<gc_vcs::ContractSnapshot, String> {
    let snap = gc_vcs::Snapshot::from_term(t).map_err(|e| e.to_string())?;
    match snap.kind {
        gc_vcs::SnapshotKind::Contract(c) => Ok(c),
        _ => Err("expected :vcs/snapshot with :kind :contract".to_string()),
    }
}

pub(super) fn vcs_diff_patch_term(
    store: &ArtifactStore,
    base_t: &Term,
    to_t: &Term,
) -> Result<(Term, Vec<String>), EffectsError> {
    fn store_value(store: &ArtifactStore, v: &Term) -> Result<String, EffectsError> {
        store.put_bytes(print_term(v).as_bytes())
    }

    fn mk_op(op_sym: &str, path: &[gc_vcs::PathStep], value: Option<&str>) -> Term {
        let mut m = BTreeMap::new();
        m.insert(TermOrdKey(Term::symbol(":op")), Term::symbol(op_sym));
        m.insert(
            TermOrdKey(Term::symbol(":path")),
            gc_vcs::path_to_term(path),
        );
        if let Some(vh) = value {
            m.insert(
                TermOrdKey(Term::symbol(":value")),
                Term::Str(vh.to_string()),
            );
        }
        Term::Map(m)
    }

    fn diff_rec(
        store: &ArtifactStore,
        path: &mut Vec<gc_vcs::PathStep>,
        a: &Term,
        b: &Term,
        ops: &mut Vec<Term>,
        values: &mut Vec<String>,
    ) -> Result<(), EffectsError> {
        if a == b {
            return Ok(());
        }
        match (a, b) {
            (Term::Map(ma), Term::Map(mb)) => {
                let mut keys: std::collections::BTreeSet<TermOrdKey> =
                    std::collections::BTreeSet::new();
                keys.extend(ma.keys().cloned());
                keys.extend(mb.keys().cloned());
                for k in keys {
                    let av = ma.get(&k);
                    let bv = mb.get(&k);
                    match (av, bv) {
                        (Some(x), Some(y)) => {
                            path.push(gc_vcs::PathStep::Map(k.0.clone()));
                            diff_rec(store, path, x, y, ops, values)?;
                            path.pop();
                        }
                        (None, Some(y)) => {
                            let vh = store_value(store, y)?;
                            values.push(vh.clone());
                            let mut p2 = path.clone();
                            p2.push(gc_vcs::PathStep::Map(k.0.clone()));
                            ops.push(mk_op(":insert", &p2, Some(&vh)));
                        }
                        (Some(_), None) => {
                            let mut p2 = path.clone();
                            p2.push(gc_vcs::PathStep::Map(k.0.clone()));
                            ops.push(mk_op(":delete", &p2, None));
                        }
                        (None, None) => {}
                    }
                }
                Ok(())
            }
            (Term::Vector(_), Term::Vector(_))
            | (Term::Pair(_, _), Term::Pair(_, _))
            | (Term::Vector(_), _)
            | (_, Term::Vector(_))
            | (Term::Pair(_, _), _)
            | (_, Term::Pair(_, _)) => {
                // Conservative: replace whole node when shape differs or container contents differ.
                let vh = store_value(store, b)?;
                values.push(vh.clone());
                ops.push(mk_op(":replace", path, Some(&vh)));
                Ok(())
            }
            _ => {
                let vh = store_value(store, b)?;
                values.push(vh.clone());
                ops.push(mk_op(":replace", path, Some(&vh)));
                Ok(())
            }
        }
    }

    let mut ops: Vec<Term> = Vec::new();
    let mut values: Vec<String> = Vec::new();
    let mut path: Vec<gc_vcs::PathStep> = Vec::new();
    diff_rec(store, &mut path, base_t, to_t, &mut ops, &mut values)?;
    ops.sort_by_cached_key(print_term);

    let patch_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/patch"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":ops")), Term::Vector(ops)),
        ]
        .into_iter()
        .collect(),
    );
    Ok((patch_term, values))
}

pub(super) fn vcs_apply_patch_term(
    store: &ArtifactStore,
    base_t: &Term,
    patch: &gc_vcs::Patch,
) -> Result<Term, String> {
    fn rename_leaf(t: &Term, from: &str, to: &str) -> (Term, u64) {
        match t {
            Term::Symbol(s) if s == from => (Term::Symbol(to.to_string()), 1),
            Term::Str(s) if s == from => (Term::Str(to.to_string()), 1),
            _ => (t.clone(), 0),
        }
    }

    fn rename_term(t: &Term, from: &str, to: &str) -> Result<(Term, u64), String> {
        match t {
            Term::Map(m) => {
                let mut out: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
                let mut renamed: u64 = 0;
                for (k, v) in m {
                    let (nk, kc) = rename_leaf(&k.0, from, to);
                    let (nv, vc) = rename_term(v, from, to)?;
                    let nk_ord = TermOrdKey(nk);
                    if out.insert(nk_ord.clone(), nv).is_some() {
                        return Err(format!(
                            "patch rename collision for key {}",
                            print_term(&nk_ord.0)
                        ));
                    }
                    renamed = renamed.saturating_add(kc).saturating_add(vc);
                }
                Ok((Term::Map(out), renamed))
            }
            Term::Vector(xs) => {
                let mut out: Vec<Term> = Vec::with_capacity(xs.len());
                let mut renamed: u64 = 0;
                for x in xs {
                    let (nx, n) = rename_term(x, from, to)?;
                    out.push(nx);
                    renamed = renamed.saturating_add(n);
                }
                Ok((Term::Vector(out), renamed))
            }
            Term::Pair(a, d) => {
                let (na, an) = rename_term(a, from, to)?;
                let (nd, dn) = rename_term(d, from, to)?;
                Ok((
                    Term::Pair(Box::new(na), Box::new(nd)),
                    an.saturating_add(dn),
                ))
            }
            _ => Ok(rename_leaf(t, from, to)),
        }
    }

    fn update_at(
        t: &Term,
        path: &[gc_vcs::PathStep],
        f: &dyn Fn(&Term) -> Result<Term, String>,
    ) -> Result<Term, String> {
        if path.is_empty() {
            return f(t);
        }
        match &path[0] {
            gc_vcs::PathStep::Map(k) => {
                let Term::Map(m) = t else {
                    return Err("expected map".to_string());
                };
                let kk = TermOrdKey(k.clone());
                let child = m
                    .get(&kk)
                    .ok_or_else(|| format!("missing map key {}", print_term(k)))?;
                let new_child = update_at(child, &path[1..], f)?;
                let mut mm = m.clone();
                mm.insert(kk, new_child);
                Ok(Term::Map(mm))
            }
            gc_vcs::PathStep::Vec(i) | gc_vcs::PathStep::Form(i) => {
                let Term::Vector(xs) = t else {
                    return Err("expected vector".to_string());
                };
                if *i >= xs.len() {
                    return Err(format!("vector index out of range: {i}"));
                }
                let mut ys = xs.clone();
                let new_child = update_at(&ys[*i], &path[1..], f)?;
                ys[*i] = new_child;
                Ok(Term::Vector(ys))
            }
            gc_vcs::PathStep::PairCar => {
                let Term::Pair(a, d) = t else {
                    return Err("expected pair".to_string());
                };
                let new_a = update_at(a, &path[1..], f)?;
                Ok(Term::Pair(Box::new(new_a), d.clone()))
            }
            gc_vcs::PathStep::PairCdr => {
                let Term::Pair(a, d) = t else {
                    return Err("expected pair".to_string());
                };
                let new_d = update_at(d, &path[1..], f)?;
                Ok(Term::Pair(a.clone(), Box::new(new_d)))
            }
        }
    }

    fn replace_at(t: &Term, path: &[gc_vcs::PathStep], new_term: Term) -> Result<Term, String> {
        update_at(t, path, &|_cur| Ok(new_term.clone()))
    }

    fn insert_at(t: &Term, path: &[gc_vcs::PathStep], new_term: Term) -> Result<Term, String> {
        let (last, parent) = path.split_last().ok_or_else(|| "empty path".to_string())?;
        update_at(t, parent, &|cur| match last {
            gc_vcs::PathStep::Map(k) => {
                let Term::Map(m) = cur else {
                    return Err("expected map".to_string());
                };
                let kk = TermOrdKey(k.clone());
                if m.contains_key(&kk) {
                    return Err(format!("map key already present {}", print_term(k)));
                }
                let mut mm = m.clone();
                mm.insert(kk, new_term.clone());
                Ok(Term::Map(mm))
            }
            gc_vcs::PathStep::Vec(i) | gc_vcs::PathStep::Form(i) => {
                let Term::Vector(xs) = cur else {
                    return Err("expected vector".to_string());
                };
                if *i > xs.len() {
                    return Err(format!("vector insert index out of range: {i}"));
                }
                let mut ys = xs.clone();
                ys.insert(*i, new_term.clone());
                Ok(Term::Vector(ys))
            }
            _ => Err("insert requires :map or :vec/:form final step".to_string()),
        })
    }

    fn delete_at(t: &Term, path: &[gc_vcs::PathStep]) -> Result<Term, String> {
        let (last, parent) = path.split_last().ok_or_else(|| "empty path".to_string())?;
        update_at(t, parent, &|cur| match last {
            gc_vcs::PathStep::Map(k) => {
                let Term::Map(m) = cur else {
                    return Err("expected map".to_string());
                };
                let kk = TermOrdKey(k.clone());
                if !m.contains_key(&kk) {
                    return Err(format!("missing map key {}", print_term(k)));
                }
                let mut mm = m.clone();
                mm.remove(&kk);
                Ok(Term::Map(mm))
            }
            gc_vcs::PathStep::Vec(i) | gc_vcs::PathStep::Form(i) => {
                let Term::Vector(xs) = cur else {
                    return Err("expected vector".to_string());
                };
                if *i >= xs.len() {
                    return Err(format!("vector index out of range: {i}"));
                }
                let mut ys = xs.clone();
                ys.remove(*i);
                Ok(Term::Vector(ys))
            }
            _ => Err("delete requires :map or :vec/:form final step".to_string()),
        })
    }

    let mut cur = base_t.clone();
    for opx in &patch.ops {
        match opx {
            gc_vcs::PatchOp::Replace { path, value } => {
                let vterm = store_get_term(store, value)
                    .map_err(|e| format!("patch value read error: {e}"))?;
                cur = replace_at(&cur, path, vterm)?;
            }
            gc_vcs::PatchOp::Insert { path, value } => {
                let vterm = store_get_term(store, value)
                    .map_err(|e| format!("patch value read error: {e}"))?;
                cur = insert_at(&cur, path, vterm)?;
            }
            gc_vcs::PatchOp::Delete { path } => {
                cur = delete_at(&cur, path)?;
            }
            gc_vcs::PatchOp::Rename { from, to } => {
                if from == to {
                    continue;
                }
                let (next, renamed) = rename_term(&cur, from, to)?;
                if renamed == 0 {
                    return Err(format!("patch rename target not found: {from}"));
                }
                cur = next;
            }
        }
    }
    Ok(cur)
}

pub(super) fn mk_conflict_artifact(
    kind: &str,
    base: &str,
    left: &str,
    right: &str,
    conflicts: Vec<Term>,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/conflict"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":kind")), Term::symbol(kind)),
            (
                TermOrdKey(Term::symbol(":base")),
                Term::Str(base.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":left")),
                Term::Str(left.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":right")),
                Term::Str(right.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":conflicts")),
                Term::Vector(conflicts),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

#[derive(Debug, Clone)]
pub(super) enum Selector {
    Commit(String),
    Snapshot(String),
    Ref(String),
}

pub(super) fn parse_selector(s: &str) -> Option<Selector> {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("commit:") {
        return Some(Selector::Commit(rest.trim().to_string()));
    }
    if let Some(rest) = t.strip_prefix("snapshot:") {
        return Some(Selector::Snapshot(rest.trim().to_string()));
    }
    if let Some(rest) = t.strip_prefix("ref:") {
        return Some(Selector::Ref(rest.trim().to_string()));
    }
    if t.starts_with("refs/") {
        return Some(Selector::Ref(t.to_string()));
    }
    if gc_vcs::validate_hex_hash(t).is_ok() {
        return Some(Selector::Commit(t.to_string()));
    }
    None
}

pub(super) fn compute_requirement_fingerprint(
    req: &gc_pkg::Requirement,
    snapshot: Option<&str>,
    commit: Option<&str>,
) -> String {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":selector")),
        Term::Str(req.selector.clone()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":update-policy")),
        Term::Symbol(match req.update_policy {
            gc_pkg::UpdatePolicy::Manual => ":manual".to_string(),
            gc_pkg::UpdatePolicy::Auto => ":auto".to_string(),
        }),
    );
    m.insert(
        TermOrdKey(Term::symbol(":strategy")),
        Term::Symbol(format!(":{}", req.strategy.as_str())),
    );
    m.insert(
        TermOrdKey(Term::symbol(":tag-policy")),
        req.tag_policy.clone().map(Term::Str).unwrap_or(Term::Nil),
    );
    m.insert(
        TermOrdKey(Term::symbol(":registry")),
        req.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
    );
    m.insert(
        TermOrdKey(Term::symbol(":snapshot")),
        snapshot
            .map(|s| Term::Str(s.to_string()))
            .unwrap_or(Term::Nil),
    );
    m.insert(
        TermOrdKey(Term::symbol(":commit")),
        commit
            .map(|s| Term::Str(s.to_string()))
            .unwrap_or(Term::Nil),
    );
    blake3::hash((print_term(&Term::Map(m)) + "\n").as_bytes())
        .to_hex()
        .to_string()
}

pub(super) fn validate_commit_artifact_closure(
    store: &ArtifactStore,
    dep_name: &str,
    snapshot_hex: &str,
    commit_hex: &str,
    require_evidence_for_obligations: bool,
    error_tok: SealId,
    op: &str,
) -> Result<u64, Value> {
    let mut checked: u64 = 0;
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut ensure_hash = |h: &str| -> Result<(), Value> {
        if !store.path_for(h).exists() {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {h}"),
                Some(op),
            ));
        }
        if store.verify_hex(h).is_err() {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact store corruption: {h}"),
                Some(op),
            ));
        }
        if seen.insert(h.to_string()) {
            checked = checked.saturating_add(1);
        }
        Ok(())
    };

    ensure_hash(commit_hex)?;
    let commit_term = match store_get_term(store, commit_hex) {
        Ok(t) => t,
        Err(_) => {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {commit_hex}"),
                Some(op),
            ));
        }
    };
    let c = match gc_vcs::Commit::from_term(&commit_term) {
        Ok(c) => c,
        Err(e) => {
            return Err(mk_error(
                error_tok,
                "core/pkg/bad-commit",
                e.to_string(),
                Some(op),
            ));
        }
    };
    if c.result != snapshot_hex {
        return Err(mk_error(
            error_tok,
            "core/pkg/commit-snapshot-mismatch",
            format!("commit.result != locked.snapshot for {dep_name}"),
            Some(op),
        ));
    }
    if let Some(base) = c.base.as_deref() {
        ensure_hash(base)?;
    }
    ensure_hash(&c.patch)?;
    ensure_hash(&c.result)?;

    if require_evidence_for_obligations && !c.obligations.is_empty() && c.evidence.is_empty() {
        return Err(mk_error(
            error_tok,
            "core/pkg/missing-evidence",
            format!("commit has obligations but no evidence for {dep_name}"),
            Some(op),
        ));
    }

    for evh in &c.evidence {
        ensure_hash(evh)?;
        let ev_term = match store_get_term(store, evh) {
            Ok(t) => t,
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/pkg/bad-evidence",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        if let Err(e) = gc_vcs::Evidence::from_term(&ev_term) {
            return Err(mk_error(
                error_tok,
                "core/pkg/bad-evidence",
                e.to_string(),
                Some(op),
            ));
        }
    }
    for at_h in &c.attestations {
        ensure_hash(at_h)?;
        let at_term = match store_get_term(store, at_h) {
            Ok(t) => t,
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/pkg/bad-attestation",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        if let Err(e) = gc_vcs::Attestation::from_term(&at_term) {
            return Err(mk_error(
                error_tok,
                "core/pkg/bad-attestation",
                e.to_string(),
                Some(op),
            ));
        }
    }
    Ok(checked)
}

pub(super) fn validate_locked_entries_strict(
    store: &ArtifactStore,
    requirements: &BTreeMap<String, gc_pkg::Requirement>,
    locked: &BTreeMap<String, gc_pkg::LockedEntry>,
    require_evidence_for_obligations: bool,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    for (name, le) in locked {
        let req = requirements.get(name).ok_or_else(|| {
            mk_error(
                error_tok,
                "core/pkg/lock-invariant",
                format!("missing requirement entry for locked dependency: {name}"),
                Some(op),
            )
        })?;

        if le.source_selector != req.selector {
            return Err(mk_error(
                error_tok,
                "core/pkg/lock-invariant",
                format!("locked.source_selector mismatch for {name}"),
                Some(op),
            ));
        }

        let inferred_strategy = gc_pkg::infer_strategy(&req.selector);
        if req.strategy != inferred_strategy {
            return Err(mk_error(
                error_tok,
                "core/pkg/lock-invariant",
                format!(
                    "selector strategy mismatch for {name} (declared={}, inferred={})",
                    req.strategy.as_str(),
                    inferred_strategy.as_str()
                ),
                Some(op),
            ));
        }
        if matches!(req.strategy, gc_pkg::ResolutionStrategy::TagPolicy) && req.tag_policy.is_none()
        {
            return Err(mk_error(
                error_tok,
                "core/pkg/lock-invariant",
                format!("tag-policy strategy requires tag_policy for {name}"),
                Some(op),
            ));
        }
        if !matches!(req.strategy, gc_pkg::ResolutionStrategy::TagPolicy)
            && req.tag_policy.is_some()
        {
            return Err(mk_error(
                error_tok,
                "core/pkg/lock-invariant",
                format!("tag_policy is only valid for tag-policy strategy: {name}"),
                Some(op),
            ));
        }

        match parse_selector(&req.selector) {
            Some(Selector::Snapshot(_)) => {
                if le.resolved_ref.is_some() {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!("snapshot selector must not set resolved_ref for {name}"),
                        Some(op),
                    ));
                }
            }
            Some(Selector::Commit(sel_h)) => {
                if le.resolved_ref.is_some() {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!("commit selector must not set resolved_ref for {name}"),
                        Some(op),
                    ));
                }
                let Some(locked_commit) = &le.commit else {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!("commit selector resolved without commit for {name}"),
                        Some(op),
                    ));
                };
                if !locked_commit.eq_ignore_ascii_case(&sel_h) {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!("commit selector hash mismatch for {name}"),
                        Some(op),
                    ));
                }
            }
            Some(Selector::Ref(ref_name)) => {
                if le.resolved_ref.as_deref() != Some(ref_name.as_str()) {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!("ref selector resolved_ref mismatch for {name}"),
                        Some(op),
                    ));
                }
                if le.commit.is_none() {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!("ref selector resolved without commit for {name}"),
                        Some(op),
                    ));
                }
            }
            None => {
                return Err(mk_error(
                    error_tok,
                    "core/pkg/bad-selector",
                    format!("unsupported selector: {}", req.selector),
                    Some(op),
                ));
            }
        }

        if let Some(fp) = &le.environment_fingerprint {
            let expected_fp =
                compute_requirement_fingerprint(req, Some(&le.snapshot), le.commit.as_deref());
            if fp != &expected_fp {
                return Err(mk_error(
                    error_tok,
                    "core/pkg/lock-invariant",
                    format!("environment_fingerprint mismatch for {name}"),
                    Some(op),
                ));
            }
        }

        if !store.path_for(&le.snapshot).exists() {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {}", le.snapshot),
                Some(op),
            ));
        }
        if store.verify_hex(&le.snapshot).is_err() {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact store corruption: {}", le.snapshot),
                Some(op),
            ));
        }
        let snap_term = match store_get_term(store, &le.snapshot) {
            Ok(t) => t,
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/pkg/bad-snapshot",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        if let Err(e) = gc_vcs::Snapshot::from_term(&snap_term) {
            return Err(mk_error(
                error_tok,
                "core/pkg/bad-snapshot",
                e.to_string(),
                Some(op),
            ));
        }

        if let Some(commit_hex) = &le.commit
            && let Err(v) = validate_commit_artifact_closure(
                store,
                name,
                &le.snapshot,
                commit_hex,
                require_evidence_for_obligations,
                error_tok,
                op,
            )
        {
            return Err(v);
        }
    }
    Ok(())
}

pub(super) fn workspace_snapshot_term_from_lock(lock: &gc_pkg::GenesisLock) -> Term {
    let modules = lock
        .locked
        .iter()
        .map(|(name, le)| {
            (
                TermOrdKey(Term::Str(name.clone())),
                Term::Str(le.snapshot.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/snapshot"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":workspace"),
            ),
            (
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(lock.workspace.clone()),
            ),
            (TermOrdKey(Term::symbol(":lock")), Term::Nil),
            (TermOrdKey(Term::symbol(":modules")), Term::Map(modules)),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn persist_workspace_root_snapshot(
    store: &ArtifactStore,
    lock: &gc_pkg::GenesisLock,
    error_tok: SealId,
    op: &str,
) -> Result<String, Value> {
    let snapshot_term = workspace_snapshot_term_from_lock(lock);
    let snapshot = gc_vcs::Snapshot::from_term(&snapshot_term).map_err(|e| {
        mk_error(
            error_tok,
            "core/pkg/bad-snapshot",
            format!("workspace snapshot schema error: {e}"),
            Some(op),
        )
    })?;
    match snapshot.kind {
        gc_vcs::SnapshotKind::Workspace(_) => {}
        _ => {
            return Err(mk_error(
                error_tok,
                "core/pkg/bad-snapshot",
                "workspace root snapshot must have kind :workspace".to_string(),
                Some(op),
            ));
        }
    }
    store
        .put_bytes(print_term(&snapshot_term).as_bytes())
        .map_err(|e| mk_error(error_tok, "core/store/io-error", e.to_string(), Some(op)))
}

pub(super) fn locked_dependency_provenance(
    store: &ArtifactStore,
    locked: &BTreeMap<String, gc_pkg::LockedEntry>,
    strict: bool,
    error_tok: SealId,
    op: &str,
) -> Result<Vec<Term>, Value> {
    let mut out: Vec<Term> = Vec::with_capacity(locked.len());
    for (name, le) in locked {
        let mut evidence: Vec<Term> = Vec::new();
        let mut obligations: Vec<Term> = Vec::new();
        if let Some(commit_hex) = &le.commit {
            match store_get_term(store, commit_hex).and_then(|t| {
                gc_vcs::Commit::from_term(&t)
                    .map_err(|e| EffectsError::Log(format!("bad commit: {e}")))
            }) {
                Ok(c) => {
                    evidence.extend(c.evidence.into_iter().map(Term::Str));
                    obligations.extend(c.obligations.into_iter().map(Term::Str));
                }
                Err(e) if strict => {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/bad-commit",
                        format!("{name}: {e}"),
                        Some(op),
                    ));
                }
                Err(_) => {}
            }
        }
        out.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":name")), Term::Str(name.clone())),
                (
                    TermOrdKey(Term::symbol(":snapshot")),
                    Term::Str(le.snapshot.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":commit")),
                    le.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":evidence")),
                    Term::Vector(evidence),
                ),
                (
                    TermOrdKey(Term::symbol(":obligations")),
                    Term::Vector(obligations),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }
    Ok(out)
}

pub(super) fn commit_provenance_term(commit: &gc_vcs::Commit) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":parents")),
                Term::Vector(commit.parents.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":base")),
                commit.base.clone().map(Term::Str).unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":patch")),
                Term::Str(commit.patch.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":result")),
                Term::Str(commit.result.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(commit.obligations.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":evidence")),
                Term::Vector(commit.evidence.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":attestations")),
                Term::Vector(commit.attestations.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn resolve_requirement(
    store: &ArtifactStore,
    refs: &RefsDb,
    _name: &str,
    req: &gc_pkg::Requirement,
    error_tok: SealId,
    op: &str,
) -> Result<gc_pkg::LockedEntry, Value> {
    let inferred_strategy = gc_pkg::infer_strategy(&req.selector);
    if req.strategy != inferred_strategy {
        return Err(mk_error(
            error_tok,
            "core/pkg/bad-selector",
            format!(
                "selector strategy mismatch: declared {}, inferred {}",
                req.strategy.as_str(),
                inferred_strategy.as_str()
            ),
            Some(op),
        ));
    }
    if matches!(req.strategy, gc_pkg::ResolutionStrategy::TagPolicy) && req.tag_policy.is_none() {
        return Err(mk_error(
            error_tok,
            "core/pkg/bad-selector",
            "tag-policy strategy requires tag_policy".to_string(),
            Some(op),
        ));
    }
    if !matches!(req.strategy, gc_pkg::ResolutionStrategy::TagPolicy) && req.tag_policy.is_some() {
        return Err(mk_error(
            error_tok,
            "core/pkg/bad-selector",
            "tag_policy is only valid for tag-policy strategy".to_string(),
            Some(op),
        ));
    }

    let sel = parse_selector(&req.selector).ok_or_else(|| {
        mk_error(
            error_tok,
            "core/pkg/bad-selector",
            format!("unsupported selector: {}", req.selector),
            Some(op),
        )
    })?;

    match sel {
        Selector::Snapshot(h) => {
            if let Err(e) = gc_vcs::validate_hex_hash(&h) {
                return Err(mk_error(error_tok, "core/pkg/bad-selector", e, Some(op)));
            }
            let fp = compute_requirement_fingerprint(req, Some(&h), None);
            Ok(gc_pkg::LockedEntry {
                commit: None,
                snapshot: h,
                registry: req.registry.clone(),
                source_selector: req.selector.clone(),
                resolved_ref: None,
                exports_hash: None,
                environment_fingerprint: Some(fp),
            })
        }
        Selector::Commit(h) => {
            if let Err(e) = gc_vcs::validate_hex_hash(&h) {
                return Err(mk_error(error_tok, "core/pkg/bad-selector", e, Some(op)));
            }
            if !store.path_for(&h).exists() {
                return Err(mk_error(
                    error_tok,
                    "core/store/not-found",
                    format!("artifact not found: {h}"),
                    Some(op),
                ));
            }
            let t = store_get_term(store, &h)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let c = gc_vcs::Commit::from_term(&t)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let snapshot = c.result;
            let fp = compute_requirement_fingerprint(req, Some(snapshot.as_str()), Some(&h));
            Ok(gc_pkg::LockedEntry {
                commit: Some(h),
                snapshot,
                registry: req.registry.clone(),
                source_selector: req.selector.clone(),
                resolved_ref: None,
                exports_hash: None,
                environment_fingerprint: Some(fp),
            })
        }
        Selector::Ref(rn) => {
            let h = refs
                .get(&rn)
                .map_err(|e| mk_error(error_tok, "core/refs/io-error", e.to_string(), Some(op)))?;
            let Some(commit_hex) = h else {
                return Err(mk_error(
                    error_tok,
                    "core/pkg/ref-not-found",
                    format!("ref not found: {rn}"),
                    Some(op),
                ));
            };
            if !store.path_for(&commit_hex).exists() {
                return Err(mk_error(
                    error_tok,
                    "core/store/not-found",
                    format!("artifact not found: {commit_hex}"),
                    Some(op),
                ));
            }
            let t = store_get_term(store, &commit_hex)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let c = gc_vcs::Commit::from_term(&t)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let snapshot = c.result;
            let fp =
                compute_requirement_fingerprint(req, Some(snapshot.as_str()), Some(&commit_hex));
            Ok(gc_pkg::LockedEntry {
                commit: Some(commit_hex),
                snapshot,
                registry: req.registry.clone(),
                source_selector: req.selector.clone(),
                resolved_ref: Some(rn),
                exports_hash: None,
                environment_fingerprint: Some(fp),
            })
        }
    }
}
