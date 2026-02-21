use super::*;

pub(crate) fn hash_bytes_hex(bytes: &[u8]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(bytes);
    h.finalize().to_hex().to_string()
}

pub(crate) fn as_contract_snapshot(t: &Term) -> Result<gc_vcs::ContractSnapshot, String> {
    let snap = gc_vcs::Snapshot::from_term(t).map_err(|e| e.to_string())?;
    match snap.kind {
        gc_vcs::SnapshotKind::Contract(c) => Ok(c),
        _ => Err("expected :vcs/snapshot with :kind :contract".to_string()),
    }
}

pub(crate) fn vcs_diff_patch_term(
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

pub(crate) fn vcs_apply_patch_term(
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

pub(crate) fn mk_conflict_artifact(
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
