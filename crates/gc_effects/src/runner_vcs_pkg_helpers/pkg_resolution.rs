use super::*;
use crate::refs::RefEntry;
use semver::{Version, VersionReq};

#[path = "pkg_resolution/lock_validation.rs"]
mod lock_validation;

pub(crate) use lock_validation::{
    commit_provenance_term, compute_requirement_fingerprint, locked_dependency_provenance,
    persist_workspace_root_snapshot, validate_commit_artifact_closure,
    validate_locked_entries_strict,
};

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
