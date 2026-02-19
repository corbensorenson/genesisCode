use super::*;

pub(super) fn cap_store_put(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let store = store
        .ok_or_else(|| EffectsError::Log("missing artifact store for core/store::put".to_string()))?;
    let art = payload_store_artifact(payload)?;
    let bytes = print_term(&art);
    let configured_max = match op_extra_positive_usize(pol, "max_bytes") {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let max_bytes = effective_limit(configured_max, HARD_REMOTE_ARTIFACT_MAX_BYTES);
    if bytes.len() > max_bytes {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "store put bytes",
            bytes.len(),
            max_bytes,
        ));
    }
    let h = match store_put_with_budget(store, bytes.as_bytes(), policy, budget, error_tok, op) {
        Ok(h) => h,
        Err(v) => return Ok(v),
    };
    let mut m = BTreeMap::new();
    m.insert(TermOrdKey(Term::Symbol(":hash".to_string())), Term::Str(h));
    Ok(Value::Data(Term::Map(m)))
}

pub(super) fn cap_store_has(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    timeout_ms: Option<u64>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let store = store
        .ok_or_else(|| EffectsError::Log("missing artifact store for core/store::has".to_string()))?;
    let h = payload_store_hash(payload)?;
    let p = store.path_for(&h);
    let mut present = false;
    if p.exists() {
        if let Err(e) = store.verify_hex(&h) {
            return Ok(mk_error(
                error_tok,
                "core/store/corruption",
                e.to_string(),
                Some(op),
            ));
        }
        present = true;
    } else {
        match store_remote_client(policy, pol, timeout_ms, error_tok, op) {
            Ok(Some((client, _base))) => {
                let mp = match client.store_has(std::slice::from_ref(&h)) {
                    Ok(m) => m,
                    Err(e) => {
                        let code = match &e {
                            gc_registry::RegistryError::Auth(_) => "core/store/remote-auth",
                            _ => "core/store/remote-error",
                        };
                        return Ok(mk_error(error_tok, code, e.to_string(), Some(op)));
                    }
                };
                present = mp.get(&h).copied().unwrap_or(false);
            }
            Ok(None) => {}
            Err(v) => return Ok(v),
        }
    }
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":present".to_string())),
        Term::Bool(present),
    );
    Ok(Value::Data(Term::Map(m)))
}

pub(super) fn cap_store_get(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    budget: &mut ArtifactBudgetState,
    timeout_ms: Option<u64>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let store = store
        .ok_or_else(|| EffectsError::Log("missing artifact store for core/store::get".to_string()))?;
    let configured_max = match op_extra_positive_usize(pol, "max_bytes") {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    let max_bytes = effective_limit(configured_max, HARD_REMOTE_ARTIFACT_MAX_BYTES);
    let h = payload_store_hash(payload)?;
    let p = store.path_for(&h);
    if !p.exists() {
        match store_remote_client(policy, pol, timeout_ms, error_tok, op) {
            Ok(Some((client, _base))) => {
                let bytes = match client.store_get_opt_bounded(&h, Some(max_bytes)) {
                    Ok(Some(b)) => b,
                    Ok(None) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-found",
                            format!("artifact not found: {h}"),
                            Some(op),
                        ));
                    }
                    Err(e) => {
                        if format!("{e}").contains("resource-limit:") {
                            return Ok(mk_resource_limit_error(
                                error_tok,
                                op,
                                "remote artifact bytes",
                                max_bytes.saturating_add(1),
                                max_bytes,
                            ));
                        }
                        let code = match &e {
                            gc_registry::RegistryError::Auth(_) => "core/store/remote-auth",
                            _ => "core/store/remote-error",
                        };
                        return Ok(mk_error(error_tok, code, e.to_string(), Some(op)));
                    }
                };
                let got = hash_bytes_hex(&bytes);
                if got != h {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/hash-mismatch",
                        "remote bytes hash mismatch".to_string(),
                        Some(op),
                    ));
                }
                match store_put_with_budget(store, &bytes, policy, budget, error_tok, op) {
                    Ok(stored_h) if stored_h == h => {}
                    Ok(_) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/hash-mismatch",
                            "local store wrote under different hash".to_string(),
                            Some(op),
                        ));
                    }
                    Err(v) => return Ok(v),
                }
            }
            Ok(None) => {}
            Err(v) => return Ok(v),
        }
    }
    if !p.exists() {
        return Ok(mk_error(
            error_tok,
            "core/store/not-found",
            format!("artifact not found: {h}"),
            Some(op),
        ));
    }
    if let Err(e) = store.verify_hex(&h) {
        return Ok(mk_error(
            error_tok,
            "core/store/corruption",
            e.to_string(),
            Some(op),
        ));
    }
    if let Ok(md) = std::fs::metadata(store.path_for(&h))
        && md.len() > max_bytes as u64
    {
        return Ok(mk_resource_limit_error(
            error_tok,
            op,
            "artifact bytes",
            md.len() as usize,
            max_bytes,
        ));
    }
    let bytes = match store.get_bytes_limited(&h, max_bytes) {
        Ok(b) => b,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/store/io-error",
                e.to_string(),
                Some(op),
            ));
        }
    };
    let s = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            return Ok(mk_error(
                error_tok,
                "core/store/bad-artifact",
                "artifact bytes are not a utf-8 CoreForm term".to_string(),
                Some(op),
            ));
        }
    };
    let t = match gc_coreform::parse_term(&s) {
        Ok(t) => t,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/store/bad-artifact",
                format!("bad artifact term: {e}"),
                Some(op),
            ));
        }
    };
    let mut m = BTreeMap::new();
    m.insert(TermOrdKey(Term::Symbol(":artifact".to_string())), t);
    Ok(Value::Data(Term::Map(m)))
}

pub(super) fn cap_store_verify(
    op: &str,
    payload: &Term,
    store: Option<&ArtifactStore>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/store::verify".to_string())
    })?;
    let maybe_hash = match payload_store_optional_hash(payload) {
        Ok(h) => h,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/store/bad-payload",
                e.to_string(),
                Some(op),
            ));
        }
    };
    let mut checked: u64 = 0;
    if let Some(h) = maybe_hash.clone() {
        if !is_hex64(&h) {
            return Ok(mk_error(
                error_tok,
                "core/store/bad-hash",
                format!("invalid store hash: {h}"),
                Some(op),
            ));
        }
        if !store.path_for(&h).exists() {
            return Ok(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {h}"),
                Some(op),
            ));
        }
        checked = 1;
        if let Err(e) = store.verify_hex(&h) {
            let ctx = Term::Map(
                [
                    (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
                    (
                        TermOrdKey(Term::symbol(":checked")),
                        Term::Int(BigInt::from(checked)),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            return Ok(mk_error_with_ctx(
                error_tok,
                "core/store/corruption",
                e.to_string(),
                Some(op),
                ctx,
            ));
        }
    } else {
        let hashes = match store_scan_hashes(store) {
            Ok(hs) => hs,
            Err(e) => {
                return Ok(mk_error(
                    error_tok,
                    "core/store/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        for h in hashes {
            checked = checked.saturating_add(1);
            if let Err(e) = store.verify_hex(&h) {
                let ctx = Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
                        (
                            TermOrdKey(Term::symbol(":checked")),
                            Term::Int(BigInt::from(checked)),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                return Ok(mk_error_with_ctx(
                    error_tok,
                    "core/store/corruption",
                    e.to_string(),
                    Some(op),
                    ctx,
                ));
            }
        }
    }
    let mut out = BTreeMap::new();
    out.insert(
        TermOrdKey(Term::symbol(":checked")),
        Term::Int(BigInt::from(checked)),
    );
    out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    if let Some(h) = maybe_hash {
        out.insert(TermOrdKey(Term::symbol(":hash")), Term::Str(h));
    }
    Ok(Value::Data(Term::Map(out)))
}
