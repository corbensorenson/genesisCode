use super::*;

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
