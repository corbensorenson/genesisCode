use super::*;
use crate::refs::RefEntry;
use semver::{Version, VersionReq};

#[derive(Debug, Clone)]
pub(crate) enum Selector {
    Commit(String),
    Snapshot(String),
    Ref(String),
    SemverRange(String),
}

pub(crate) fn parse_selector(s: &str) -> Option<Selector> {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("semver:") {
        let range = rest.trim();
        if range.is_empty() {
            return None;
        }
        return Some(Selector::SemverRange(range.to_string()));
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemverSelectionPolicy {
    Highest,
    Lowest,
}

fn semver_selection_policy(tag_policy: Option<&str>) -> Result<SemverSelectionPolicy, String> {
    match tag_policy.unwrap_or("highest") {
        // Keep existing tag-policy defaults backward compatible with v0.1 ("exact").
        "highest" | "latest" | "exact" => Ok(SemverSelectionPolicy::Highest),
        "lowest" => Ok(SemverSelectionPolicy::Lowest),
        other => Err(format!(
            "unsupported semver tag_policy `{other}` (expected highest|lowest)"
        )),
    }
}

fn parse_tag_semver_version(ref_name: &str) -> Option<Version> {
    let tag = ref_name.strip_prefix("refs/tags/")?;
    if tag.is_empty() {
        return None;
    }
    Version::parse(tag).ok().or_else(|| {
        tag.strip_prefix('v')
            .and_then(|raw| Version::parse(raw).ok())
    })
}

fn select_semver_tag_ref(
    refs: &[RefEntry],
    req: &VersionReq,
    policy: SemverSelectionPolicy,
) -> Option<(String, String)> {
    let mut best: Option<(String, String, Version)> = None;
    for entry in refs {
        let Some(commit_hex) = entry.hash.as_ref() else {
            continue;
        };
        let Some(version) = parse_tag_semver_version(&entry.name) else {
            continue;
        };
        if !req.matches(&version) {
            continue;
        }
        let candidate = (entry.name.clone(), commit_hex.clone(), version);
        let replace = match &best {
            None => true,
            Some((best_ref, _best_commit, best_version)) => match policy {
                SemverSelectionPolicy::Highest => {
                    candidate.2 > *best_version
                        || (candidate.2 == *best_version && candidate.0 < *best_ref)
                }
                SemverSelectionPolicy::Lowest => {
                    candidate.2 < *best_version
                        || (candidate.2 == *best_version && candidate.0 < *best_ref)
                }
            },
        };
        if replace {
            best = Some(candidate);
        }
    }
    best.map(|(ref_name, commit_hex, _)| (ref_name, commit_hex))
}

fn collect_available_semver_tags(refs: &[RefEntry]) -> Vec<Term> {
    let mut tags: Vec<String> = refs
        .iter()
        .filter_map(|entry| {
            parse_tag_semver_version(&entry.name)?;
            Some(entry.name.clone())
        })
        .collect();
    tags.sort();
    tags.dedup();
    tags.into_iter().map(Term::Str).collect()
}

pub(crate) fn compute_requirement_fingerprint(
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

pub(crate) fn validate_commit_artifact_closure(
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

pub(crate) fn validate_locked_entries_strict(
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
            Some(Selector::SemverRange(range)) => {
                let Some(resolved_ref) = le.resolved_ref.as_deref() else {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!("semver selector resolved without resolved_ref for {name}"),
                        Some(op),
                    ));
                };
                if !resolved_ref.starts_with("refs/tags/") {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!("semver selector resolved_ref is not under refs/tags/* for {name}"),
                        Some(op),
                    ));
                }
                let req_range = VersionReq::parse(&range).map_err(|e| {
                    mk_error(
                        error_tok,
                        "core/pkg/bad-selector",
                        format!("invalid semver selector range `{range}`: {e}"),
                        Some(op),
                    )
                })?;
                let Some(resolved_version) = parse_tag_semver_version(resolved_ref) else {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!("resolved semver tag is not parseable for {name}: {resolved_ref}"),
                        Some(op),
                    ));
                };
                if !req_range.matches(&resolved_version) {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!(
                            "resolved semver tag `{resolved_ref}` is outside selector range `{range}` for {name}"
                        ),
                        Some(op),
                    ));
                }
                if le.commit.is_none() {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/lock-invariant",
                        format!("semver selector resolved without commit for {name}"),
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

pub(crate) fn workspace_snapshot_term_from_lock(lock: &gc_pkg::GenesisLock) -> Term {
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

pub(crate) fn persist_workspace_root_snapshot(
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

pub(crate) fn locked_dependency_provenance(
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

pub(crate) fn commit_provenance_term(commit: &gc_vcs::Commit) -> Term {
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

pub(crate) fn resolve_requirement(
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
        Selector::SemverRange(range) => {
            let req_range = VersionReq::parse(&range).map_err(|e| {
                mk_error(
                    error_tok,
                    "core/pkg/bad-selector",
                    format!("invalid semver selector range `{range}`: {e}"),
                    Some(op),
                )
            })?;
            let policy = semver_selection_policy(req.tag_policy.as_deref())
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-selector", e, Some(op)))?;
            let refs_list = refs
                .list(Some("refs/tags/"))
                .map_err(|e| mk_error(error_tok, "core/refs/io-error", e.to_string(), Some(op)))?;
            let Some((resolved_ref, commit_hex)) =
                select_semver_tag_ref(&refs_list, &req_range, policy)
            else {
                return Err(mk_error_with_ctx(
                    error_tok,
                    "core/pkg/semver-no-match",
                    format!("no refs/tags entry satisfies semver range `{range}`"),
                    Some(op),
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":selector")),
                                Term::Str(req.selector.clone()),
                            ),
                            (TermOrdKey(Term::symbol(":range")), Term::Str(range.clone())),
                            (
                                TermOrdKey(Term::symbol(":tag-policy")),
                                req.tag_policy.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                            (
                                TermOrdKey(Term::symbol(":registry")),
                                req.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                            (
                                TermOrdKey(Term::symbol(":available-tags")),
                                Term::Vector(collect_available_semver_tags(&refs_list)),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
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
                resolved_ref: Some(resolved_ref),
                exports_hash: None,
                environment_fingerprint: Some(fp),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selector_accepts_semver_range() {
        let parsed = parse_selector("semver:^1.2.0");
        assert!(matches!(parsed, Some(Selector::SemverRange(r)) if r == "^1.2.0"));
    }

    #[test]
    fn semver_tag_selection_is_deterministic_by_policy() {
        let refs = vec![
            RefEntry {
                name: "refs/tags/v1.2.0".to_string(),
                hash: Some("a".repeat(64)),
            },
            RefEntry {
                name: "refs/tags/v1.2.3".to_string(),
                hash: Some("b".repeat(64)),
            },
            RefEntry {
                name: "refs/tags/v1.2.5".to_string(),
                hash: Some("c".repeat(64)),
            },
            RefEntry {
                name: "refs/tags/v2.0.0".to_string(),
                hash: Some("d".repeat(64)),
            },
        ];
        let range = VersionReq::parse("^1.2.0").expect("valid range");
        let high = select_semver_tag_ref(&refs, &range, SemverSelectionPolicy::Highest);
        let low = select_semver_tag_ref(&refs, &range, SemverSelectionPolicy::Lowest);
        assert_eq!(high, Some(("refs/tags/v1.2.5".to_string(), "c".repeat(64))));
        assert_eq!(low, Some(("refs/tags/v1.2.0".to_string(), "a".repeat(64))));
    }
}
