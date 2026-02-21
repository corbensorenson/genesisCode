use super::*;

pub(crate) fn vcs_load_commit(
    store: &ArtifactStore,
    commit_h: &str,
) -> Result<(gc_vcs::Commit, Term), String> {
    let t = store_get_term(store, commit_h).map_err(|e| e.to_string())?;
    let c = gc_vcs::Commit::from_term(&t).map_err(|e| format!("bad commit {commit_h}: {e}"))?;
    Ok((c, t))
}

pub(crate) fn vcs_snapshot_symbol_ref(
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

pub(crate) fn vcs_find_commit_for_snapshot(
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

pub(crate) fn vcs_blame_commit_for_symbol(
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
