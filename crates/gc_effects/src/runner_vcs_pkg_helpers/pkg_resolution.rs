use super::*;
use crate::refs::RefEntry;
use semver::{Version, VersionReq};

#[path = "pkg_resolution/lock_validation.rs"]
mod lock_validation;

#[cfg(any(test, feature = "parity-oracle"))]
pub(crate) use lock_validation::locked_dependency_provenance;
pub(crate) use lock_validation::{
    compute_requirement_fingerprint, validate_commit_artifact_closure,
    validate_locked_entries_strict,
};

#[cfg(any(test, feature = "parity-oracle"))]
fn parse_selector_parity(s: &str) -> Option<PkgResolutionSelector> {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("semver:") {
        let range = rest.trim();
        if range.is_empty() {
            return None;
        }
        return Some(PkgResolutionSelector::SemverRange(range.to_string()));
    }
    if let Some(rest) = t.strip_prefix("commit:") {
        let value = rest.trim();
        return gc_vcs::validate_hex_hash(value)
            .is_ok()
            .then(|| PkgResolutionSelector::Commit(value.to_string()));
    }
    if let Some(rest) = t.strip_prefix("snapshot:") {
        let value = rest.trim();
        return gc_vcs::validate_hex_hash(value)
            .is_ok()
            .then(|| PkgResolutionSelector::Snapshot(value.to_string()));
    }
    if let Some(rest) = t.strip_prefix("ref:") {
        let value = rest.trim();
        return value
            .starts_with("refs/")
            .then(|| PkgResolutionSelector::Ref(value.to_string()));
    }
    if t.starts_with("refs/") {
        return Some(PkgResolutionSelector::Ref(t.to_string()));
    }
    if gc_vcs::validate_hex_hash(t).is_ok() {
        return Some(PkgResolutionSelector::Commit(t.to_string()));
    }
    None
}

#[cfg(any(test, feature = "parity-oracle"))]
fn semver_selection_policy_parity(
    tag_policy: Option<&str>,
) -> Result<SemverSelectionPolicy, String> {
    match tag_policy.unwrap_or("highest") {
        // Keep existing tag-policy defaults backward compatible with v0.1 ("exact").
        "highest" | "latest" | "exact" => Ok(SemverSelectionPolicy::Highest),
        "lowest" => Ok(SemverSelectionPolicy::Lowest),
        other => Err(format!(
            "unsupported semver tag_policy `{other}` (expected highest|lowest)"
        )),
    }
}

pub(crate) fn plan_requirement(
    authority: Option<&mut PkgResolutionIdentityAuthority>,
    req: &gc_pkg::Requirement,
    has_existing: bool,
    error_tok: SealId,
    op: &str,
) -> Result<PkgResolutionPlan, Value> {
    plan_requirement_with_diagnostic(authority, req, has_existing, false, error_tok, op)
}

pub(crate) fn plan_requirement_for_strict_validation(
    authority: Option<&mut PkgResolutionIdentityAuthority>,
    req: &gc_pkg::Requirement,
    error_tok: SealId,
    op: &str,
) -> Result<PkgResolutionPlan, Value> {
    plan_requirement_with_diagnostic(authority, req, true, true, error_tok, op)
}

fn plan_requirement_with_diagnostic(
    authority: Option<&mut PkgResolutionIdentityAuthority>,
    req: &gc_pkg::Requirement,
    has_existing: bool,
    strict_validation: bool,
    error_tok: SealId,
    op: &str,
) -> Result<PkgResolutionPlan, Value> {
    if let Some(authority) = authority {
        return authority
            .plan(req, has_existing)
            .map_err(|error| match error {
                PkgResolutionPlanError::Rejected { code, message } => mk_error(
                    error_tok,
                    plan_rejection_diagnostic(&code, strict_validation),
                    message,
                    Some(op),
                ),
                PkgResolutionPlanError::Boundary(error) => mk_error(
                    error_tok,
                    "core/pkg/authority-error",
                    error.to_string(),
                    Some(op),
                ),
            });
    }

    #[cfg(any(test, feature = "parity-oracle"))]
    return plan_requirement_parity(req, has_existing).map_err(|(class, message)| {
        mk_error(
            error_tok,
            plan_rejection_diagnostic(class, strict_validation),
            message,
            Some(op),
        )
    });

    #[cfg(not(any(test, feature = "parity-oracle")))]
    Err(mk_error(
        error_tok,
        "core/pkg/authority-error",
        "package resolution requires the artifact-loaded GenesisCode planning authority"
            .to_string(),
        Some(op),
    ))
}

fn plan_rejection_diagnostic(class: &str, strict_validation: bool) -> &'static str {
    if strict_validation
        && matches!(
            class,
            "core/pkg/strategy-mismatch"
                | "core/pkg/tag-policy-required"
                | "core/pkg/tag-policy-forbidden"
        )
    {
        "core/pkg/lock-invariant"
    } else {
        "core/pkg/bad-selector"
    }
}

#[cfg(any(test, feature = "parity-oracle"))]
fn plan_requirement_parity(
    req: &gc_pkg::Requirement,
    has_existing: bool,
) -> Result<PkgResolutionPlan, (&'static str, String)> {
    let selector = parse_selector_parity(&req.selector).ok_or_else(|| {
        (
            "core/pkg/bad-selector",
            format!("unsupported selector: {}", req.selector),
        )
    })?;
    let inferred = gc_pkg::infer_strategy(&req.selector);
    if req.strategy != inferred {
        return Err((
            "core/pkg/strategy-mismatch",
            format!(
                "selector strategy mismatch: declared {}, inferred {}",
                req.strategy.as_str(),
                inferred.as_str()
            ),
        ));
    }
    if matches!(inferred, gc_pkg::ResolutionStrategy::TagPolicy) && req.tag_policy.is_none() {
        return Err((
            "core/pkg/tag-policy-required",
            "tag-policy strategy requires tag_policy".to_string(),
        ));
    }
    if !matches!(inferred, gc_pkg::ResolutionStrategy::TagPolicy) && req.tag_policy.is_some() {
        return Err((
            "core/pkg/tag-policy-forbidden",
            "tag_policy is only valid for tag-policy strategy".to_string(),
        ));
    }
    let semver_policy = if matches!(selector, PkgResolutionSelector::SemverRange(_)) {
        Some(
            semver_selection_policy_parity(req.tag_policy.as_deref())
                .map_err(|message| ("core/pkg/semver-policy-unsupported", message))?,
        )
    } else {
        None
    };
    let should_resolve = !has_existing
        || (req.update_policy == gc_pkg::UpdatePolicy::Auto
            && !matches!(inferred, gc_pkg::ResolutionStrategy::Pinned));
    Ok(PkgResolutionPlan {
        selector,
        semver_policy,
        should_resolve,
    })
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

fn collect_semver_candidates(refs: &[RefEntry], req: &VersionReq) -> Vec<PkgSemverCandidate> {
    let mut parsed = Vec::new();
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
        parsed.push((entry.name.clone(), commit_hex.clone(), version));
    }
    parsed.sort_by(|left, right| left.2.cmp_precedence(&right.2));
    let mut previous: Option<Version> = None;
    let mut rank = 0_u64;
    parsed
        .into_iter()
        .map(|(ref_name, commit, version)| {
            if previous
                .as_ref()
                .is_some_and(|prior| prior.cmp_precedence(&version) != std::cmp::Ordering::Equal)
            {
                // Candidate vectors are bounded to 64 entries by the authority profile.
                rank = rank.saturating_add(1);
            }
            previous = Some(version);
            PkgSemverCandidate {
                ref_name,
                commit,
                rank,
            }
        })
        .collect()
}

fn select_semver_tag_ref(
    authority: Option<&mut PkgResolutionIdentityAuthority>,
    candidates: &[PkgSemverCandidate],
    policy: SemverSelectionPolicy,
    error_tok: SealId,
    op: &str,
) -> Result<Option<(String, String)>, Value> {
    if let Some(authority) = authority {
        return authority
            .select_semver(candidates, policy)
            .map_err(|error| {
                let message = match error {
                    PkgResolutionPlanError::Rejected { code, message } => {
                        format!("{code}: {message}")
                    }
                    PkgResolutionPlanError::Boundary(error) => error.to_string(),
                };
                mk_error(error_tok, "core/pkg/authority-error", message, Some(op))
            });
    }

    #[cfg(any(test, feature = "parity-oracle"))]
    return Ok(select_semver_tag_ref_parity(candidates, policy));

    #[cfg(not(any(test, feature = "parity-oracle")))]
    Err(mk_error(
        error_tok,
        "core/pkg/authority-error",
        "semver selection requires the artifact-loaded GenesisCode authority".to_string(),
        Some(op),
    ))
}

#[cfg(any(test, feature = "parity-oracle"))]
fn select_semver_tag_ref_parity(
    candidates: &[PkgSemverCandidate],
    policy: SemverSelectionPolicy,
) -> Option<(String, String)> {
    candidates
        .iter()
        .min_by(|left, right| {
            let rank_order = match policy {
                SemverSelectionPolicy::Highest => right.rank.cmp(&left.rank),
                SemverSelectionPolicy::Lowest => left.rank.cmp(&right.rank),
            };
            rank_order.then_with(|| left.ref_name.cmp(&right.ref_name))
        })
        .map(|candidate| (candidate.ref_name.clone(), candidate.commit.clone()))
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

#[expect(
    clippy::too_many_arguments,
    reason = "requirement resolution carries explicit store/ref/policy context for deterministic lock hydration"
)]
pub(crate) fn resolve_requirement(
    store: &ArtifactStore,
    refs: &RefsDb,
    mut refs_authority: Option<&mut RefsAuthority>,
    registries: &BTreeMap<String, String>,
    policy: &CapsPolicy,
    op_pol: Option<&OpPolicy>,
    budget: &mut ArtifactBudgetState,
    timeout_ms: Option<u64>,
    _name: &str,
    req: &gc_pkg::Requirement,
    plan: PkgResolutionPlan,
    mut identity_authority: Option<&mut PkgResolutionIdentityAuthority>,
    error_tok: SealId,
    op: &str,
) -> Result<gc_pkg::LockedEntry, Value> {
    match plan.selector {
        PkgResolutionSelector::Snapshot(h) => {
            if let Err(e) = gc_vcs::validate_hex_hash(&h) {
                return Err(mk_error(error_tok, "core/pkg/bad-selector", e, Some(op)));
            }
            ensure_artifact_hash_available(
                store,
                registries,
                req.registry.as_deref(),
                policy,
                op_pol,
                budget,
                timeout_ms,
                &h,
                error_tok,
                op,
            )?;
            let fp = compute_requirement_fingerprint(
                identity_authority,
                req,
                Some(&h),
                None,
                error_tok,
                op,
            )?;
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
        PkgResolutionSelector::Commit(h) => {
            if let Err(e) = gc_vcs::validate_hex_hash(&h) {
                return Err(mk_error(error_tok, "core/pkg/bad-selector", e, Some(op)));
            }
            ensure_artifact_hash_available(
                store,
                registries,
                req.registry.as_deref(),
                policy,
                op_pol,
                budget,
                timeout_ms,
                &h,
                error_tok,
                op,
            )?;
            let t = store_get_term(store, &h)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let c = gc_vcs::Commit::from_term(&t)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let snapshot = c.result;
            ensure_artifact_hash_available(
                store,
                registries,
                req.registry.as_deref(),
                policy,
                op_pol,
                budget,
                timeout_ms,
                &snapshot,
                error_tok,
                op,
            )?;
            let fp = compute_requirement_fingerprint(
                identity_authority,
                req,
                Some(snapshot.as_str()),
                Some(&h),
                error_tok,
                op,
            )?;
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
        PkgResolutionSelector::Ref(rn) => {
            let local_h = RefsAuthority::consumer_get(refs_authority.as_deref_mut(), refs, &rn)
                .map_err(|e| mk_error(error_tok, "core/refs/io-error", e.to_string(), Some(op)))?;
            let commit_hex = if let Some(h) = local_h {
                h
            } else {
                let Some(client) = registry_client_for_requirement(
                    registries,
                    req.registry.as_deref(),
                    policy,
                    op_pol,
                    timeout_ms,
                    error_tok,
                    op,
                )?
                .map(|(client, _base)| client) else {
                    return Err(mk_error(
                        error_tok,
                        "core/pkg/ref-not-found",
                        format!("ref not found: {rn}"),
                        Some(op),
                    ));
                };
                match client.refs_get(&rn) {
                    Ok(Some(h)) => h,
                    Ok(None) => {
                        return Err(mk_error(
                            error_tok,
                            "core/pkg/ref-not-found",
                            format!("ref not found: {rn}"),
                            Some(op),
                        ));
                    }
                    Err(e) => {
                        let code = registry_error_code(&e, "core/store/remote-auth");
                        return Err(mk_error(error_tok, code, e.to_string(), Some(op)));
                    }
                }
            };
            ensure_artifact_hash_available(
                store,
                registries,
                req.registry.as_deref(),
                policy,
                op_pol,
                budget,
                timeout_ms,
                &commit_hex,
                error_tok,
                op,
            )?;
            let t = store_get_term(store, &commit_hex)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let c = gc_vcs::Commit::from_term(&t)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let snapshot = c.result;
            ensure_artifact_hash_available(
                store,
                registries,
                req.registry.as_deref(),
                policy,
                op_pol,
                budget,
                timeout_ms,
                &snapshot,
                error_tok,
                op,
            )?;
            let fp = compute_requirement_fingerprint(
                identity_authority,
                req,
                Some(snapshot.as_str()),
                Some(&commit_hex),
                error_tok,
                op,
            )?;
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
        PkgResolutionSelector::SemverRange(range) => {
            let req_range = VersionReq::parse(&range).map_err(|e| {
                mk_error(
                    error_tok,
                    "core/pkg/bad-selector",
                    format!("invalid semver selector range `{range}`: {e}"),
                    Some(op),
                )
            })?;
            let Some(selection_policy) = plan.semver_policy else {
                return Err(mk_error(
                    error_tok,
                    "core/pkg/authority-error",
                    "semver resolution plan omitted selection policy".to_string(),
                    Some(op),
                ));
            };
            let local_refs_list = RefsAuthority::consumer_list(
                refs_authority.as_deref_mut(),
                refs,
                Some("refs/tags/"),
            )
            .map_err(|e| mk_error(error_tok, "core/refs/io-error", e.to_string(), Some(op)))?;
            let local_candidates = collect_semver_candidates(&local_refs_list, &req_range);
            let mut resolved = select_semver_tag_ref(
                identity_authority.as_deref_mut(),
                &local_candidates,
                selection_policy,
                error_tok,
                op,
            )?;
            let mut available_tags = collect_available_semver_tags(&local_refs_list);
            if resolved.is_none()
                && let Some(client) = registry_client_for_requirement(
                    registries,
                    req.registry.as_deref(),
                    policy,
                    op_pol,
                    timeout_ms,
                    error_tok,
                    op,
                )?
                .map(|(client, _base)| client)
            {
                match client.refs_list(Some("refs/tags/")) {
                    Ok(remote_refs) => {
                        let remote_refs: Vec<RefEntry> = remote_refs
                            .into_iter()
                            .map(|entry| RefEntry {
                                name: entry.name,
                                hash: entry.hash,
                            })
                            .collect();
                        available_tags = collect_available_semver_tags(&remote_refs);
                        let remote_candidates = collect_semver_candidates(&remote_refs, &req_range);
                        resolved = select_semver_tag_ref(
                            identity_authority.as_deref_mut(),
                            &remote_candidates,
                            selection_policy,
                            error_tok,
                            op,
                        )?;
                    }
                    Err(e) => {
                        let code = registry_error_code(&e, "core/store/remote-auth");
                        return Err(mk_error(error_tok, code, e.to_string(), Some(op)));
                    }
                }
            }
            let Some((resolved_ref, commit_hex)) = resolved else {
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
                                Term::Vector(available_tags),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                ));
            };
            ensure_artifact_hash_available(
                store,
                registries,
                req.registry.as_deref(),
                policy,
                op_pol,
                budget,
                timeout_ms,
                &commit_hex,
                error_tok,
                op,
            )?;
            let t = store_get_term(store, &commit_hex)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let c = gc_vcs::Commit::from_term(&t)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let snapshot = c.result;
            ensure_artifact_hash_available(
                store,
                registries,
                req.registry.as_deref(),
                policy,
                op_pol,
                budget,
                timeout_ms,
                &snapshot,
                error_tok,
                op,
            )?;
            let fp = compute_requirement_fingerprint(
                identity_authority,
                req,
                Some(snapshot.as_str()),
                Some(&commit_hex),
                error_tok,
                op,
            )?;
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

fn registry_remote_for_requirement(
    registries: &BTreeMap<String, String>,
    registry_alias: Option<&str>,
    policy: &CapsPolicy,
) -> Result<Option<String>, String> {
    let store_remote = || store_remote_from_policy(policy).map(|remote| remote.map(str::to_string));
    match registry_alias {
        Some(alias) => match registries.get(alias) {
            Some(remote) => Ok(Some(remote.clone())),
            None if alias == "default" => store_remote(),
            None => Ok(None),
        },
        None => match registries.get("default") {
            Some(remote) => Ok(Some(remote.clone())),
            None => store_remote(),
        },
    }
}

fn registry_client_for_requirement(
    registries: &BTreeMap<String, String>,
    registry_alias: Option<&str>,
    policy: &CapsPolicy,
    op_pol: Option<&OpPolicy>,
    timeout_ms: Option<u64>,
    error_tok: SealId,
    op: &str,
) -> Result<Option<(gc_registry::RegistryClient, String)>, Value> {
    let Some(remote) = registry_remote_for_requirement(registries, registry_alias, policy)
        .map_err(|error| mk_error(error_tok, "core/caps/policy-error", error, Some(op)))?
    else {
        return Ok(None);
    };
    let base = store_normalize_and_check_remote(policy, op_pol, &remote)
        .map_err(|e| mk_error(error_tok, "core/store/remote-denied", e, Some(op)))?;
    let auth = store_registry_auth(policy)
        .map_err(|e| mk_error(error_tok, "core/caps/policy-error", e, Some(op)))?;
    let client = gc_registry::RegistryClient::new_with_auth(
        &base,
        timeout_ms.map(std::time::Duration::from_millis),
        auth,
    )
    .map_err(|e| {
        let code = registry_error_code(&e, "core/store/remote-auth");
        mk_error(error_tok, code, e.to_string(), Some(op))
    })?;
    Ok(Some((client, base)))
}

#[expect(
    clippy::too_many_arguments,
    reason = "artifact hydration requires explicit registry/policy/budget handles to keep sealing and budgeting local"
)]
pub(crate) fn ensure_artifact_hash_available(
    store: &ArtifactStore,
    registries: &BTreeMap<String, String>,
    registry_alias: Option<&str>,
    policy: &CapsPolicy,
    op_pol: Option<&OpPolicy>,
    budget: &mut ArtifactBudgetState,
    timeout_ms: Option<u64>,
    hash: &str,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    if store.path_for(hash).exists() {
        if let Err(e) = store.verify_hex(hash) {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                e.to_string(),
                Some(op),
            ));
        }
        return Ok(());
    }
    let Some((client, _base)) = registry_client_for_requirement(
        registries,
        registry_alias,
        policy,
        op_pol,
        timeout_ms,
        error_tok,
        op,
    )?
    else {
        return Err(mk_error(
            error_tok,
            "core/store/not-found",
            format!("artifact not found: {hash}"),
            Some(op),
        ));
    };
    let bytes = match client.store_get_opt_bounded(hash, Some(HARD_REMOTE_ARTIFACT_MAX_BYTES)) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {hash}"),
                Some(op),
            ));
        }
        Err(e) => {
            let code = registry_error_code(&e, "core/store/remote-auth");
            return Err(mk_error(error_tok, code, e.to_string(), Some(op)));
        }
    };
    let got = hash_bytes_hex(&bytes);
    if got != hash {
        return Err(mk_error(
            error_tok,
            "core/store/hash-mismatch",
            "remote bytes hash mismatch".to_string(),
            Some(op),
        ));
    }
    match store_put_with_budget(store, &bytes, policy, budget, error_tok, op) {
        Ok(stored_h) if stored_h == hash => Ok(()),
        Ok(_) => Err(mk_error(
            error_tok,
            "core/store/hash-mismatch",
            "local store wrote under different hash".to_string(),
            Some(op),
        )),
        Err(v) => Err(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_selector_parser_accepts_semver_range() {
        let parsed = parse_selector_parity("semver:^1.2.0");
        assert!(matches!(parsed, Some(PkgResolutionSelector::SemverRange(r)) if r == "^1.2.0"));
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
        let candidates = collect_semver_candidates(&refs, &range);
        let high = select_semver_tag_ref_parity(&candidates, SemverSelectionPolicy::Highest);
        let low = select_semver_tag_ref_parity(&candidates, SemverSelectionPolicy::Lowest);
        assert_eq!(high, Some(("refs/tags/v1.2.5".to_string(), "c".repeat(64))));
        assert_eq!(low, Some(("refs/tags/v1.2.0".to_string(), "a".repeat(64))));
    }

    #[test]
    fn semver_rank_ignores_build_metadata_and_uses_ref_tie_break() {
        let refs = vec![
            RefEntry {
                name: "refs/tags/v1.2.5+z".to_string(),
                hash: Some("b".repeat(64)),
            },
            RefEntry {
                name: "refs/tags/v1.2.5+a".to_string(),
                hash: Some("a".repeat(64)),
            },
            RefEntry {
                name: "refs/tags/v1.2.4".to_string(),
                hash: Some("c".repeat(64)),
            },
        ];
        let range = VersionReq::parse("^1.2.0").expect("valid range");
        let candidates = collect_semver_candidates(&refs, &range);
        let high = select_semver_tag_ref_parity(&candidates, SemverSelectionPolicy::Highest);
        assert_eq!(
            high,
            Some(("refs/tags/v1.2.5+a".to_string(), "a".repeat(64)))
        );
        assert_eq!(candidates[1].rank, candidates[2].rank);
    }

    #[test]
    fn plan_rejection_classes_preserve_route_diagnostics() {
        assert_eq!(
            plan_rejection_diagnostic("core/pkg/strategy-mismatch", false),
            "core/pkg/bad-selector"
        );
        assert_eq!(
            plan_rejection_diagnostic("core/pkg/strategy-mismatch", true),
            "core/pkg/lock-invariant"
        );
        assert_eq!(
            plan_rejection_diagnostic("core/pkg/bad-selector", true),
            "core/pkg/bad-selector"
        );
    }
}
