pub(super) fn resolve_gpk_root_for_export(
    store: &ArtifactStore,
    refs: Option<&RefsDb>,
    root_spec: &str,
    mode: GpkMode,
    error_tok: SealId,
    op: &str,
) -> Result<String, Value> {
    let mut root = root_spec.trim().to_string();
    if let Some(s) = root.strip_prefix("h:") {
        root = s.to_string();
    }
    if gc_vcs::validate_hex_hash(&root).is_ok() {
        return Ok(root.to_ascii_lowercase());
    }
    if let Some(s) = root.strip_prefix("ref:") {
        root = s.to_string();
    }
    if !root.starts_with("refs/") {
        return Err(mk_error(
            error_tok,
            "core/gpk/bad-root",
            "root must be a hash or refs/...".to_string(),
            Some(op),
        ));
    }
    let refs = refs.ok_or_else(|| {
        mk_error(
            error_tok,
            "core/gpk/missing-refs-db",
            "refs db required when root is a ref".to_string(),
            Some(op),
        )
    })?;
    let resolved = refs
        .get(&root)
        .map_err(|e| mk_error(error_tok, "core/gpk/refs-io-error", e.to_string(), Some(op)))?;
    let Some(hash) = resolved else {
        return Err(mk_error(
            error_tok,
            "core/gpk/ref-not-found",
            format!("ref not found: {root}"),
            Some(op),
        ));
    };
    let hash = hash.to_ascii_lowercase();
    let root_term = match store_get_term(store, &hash) {
        Ok(t) => t,
        Err(_) => {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {hash}"),
                Some(op),
            ));
        }
    };
    if mode == GpkMode::Shallow && gc_vcs::Snapshot::from_term(&root_term).is_err() {
        return Err(mk_error(
            error_tok,
            "core/gpk/bad-root",
            "shallow export root must resolve to a :vcs/snapshot".to_string(),
            Some(op),
        ));
    }
    Ok(hash)
}

#[derive(Copy, Clone, Debug)]
pub(super) struct GpkClosureOptions<'a> {
    pub(super) depth: u64,
    pub(super) mode: GpkMode,
    pub(super) include_evidence: GpkIncludeEvidence,
    pub(super) include_deps: GpkIncludeDeps,
    pub(super) root_snapshot_for_locked_deps: Option<&'a str>,
}
