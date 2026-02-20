use super::*;

pub(super) fn unseal_effect_request(
    v: &Value,
    effect_tok: SealId,
) -> Result<(EffectRequest, SealId), EffectsError> {
    let Value::Sealed { token, payload } = v else {
        return Err(EffectsError::BadEffectSeal);
    };
    if *token != effect_tok {
        return Err(EffectsError::BadEffectSeal);
    }
    let Value::EffectRequest(r) = payload.as_ref() else {
        return Err(EffectsError::BadEffectSeal);
    };
    Ok((r.clone(), *token))
}

pub(super) fn hash_request(op: &str, payload_h: [u8; 32], cont_h: [u8; 32]) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(b"GCv0.2\0effect-req\0");
    h.update(op.as_bytes());
    h.update(b"\0");
    h.update(&payload_h);
    h.update(&cont_h);
    *h.finalize().as_bytes()
}

pub(super) fn mk_caps_denied(error_tok: SealId, op: &str) -> Value {
    mk_error(
        error_tok,
        "core/caps/denied",
        format!("capability denied: {op}"),
        Some(op),
    )
}

pub(super) fn mk_error(error_tok: SealId, code: &str, msg: String, op: Option<&str>) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":error/code".to_string())),
        Term::Str(code.to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/message".to_string())),
        Term::Str(msg),
    );
    let mut ctxm = BTreeMap::new();
    ctxm.insert(
        TermOrdKey(Term::Symbol(":subsystem".to_string())),
        Term::Str("effects".to_string()),
    );
    if let Some(op) = op {
        m.insert(
            TermOrdKey(Term::Symbol(":error/op".to_string())),
            Term::Symbol(op.to_string()),
        );
        ctxm.insert(
            TermOrdKey(Term::Symbol(":op".to_string())),
            Term::Symbol(op.to_string()),
        );
    }
    m.insert(
        TermOrdKey(Term::Symbol(":error/context".to_string())),
        Term::Map(ctxm),
    );
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(m))),
    }
}

pub(super) fn mk_error_with_ctx(
    error_tok: SealId,
    code: &str,
    msg: String,
    op: Option<&str>,
    extra_ctx: Term,
) -> Value {
    let Value::Sealed { token, payload } = mk_error(error_tok, code, msg, op) else {
        unreachable!("mk_error must return sealed");
    };
    let Value::Data(Term::Map(mut m)) = *payload else {
        return Value::Sealed {
            token,
            payload: Box::new(Value::Data(Term::Map(BTreeMap::new()))),
        };
    };
    let mut ctxm = match m.remove(&TermOrdKey(Term::Symbol(":error/context".to_string()))) {
        Some(Term::Map(mm)) => mm,
        _ => BTreeMap::new(),
    };
    if let Term::Map(extra) = extra_ctx {
        for (k, v) in extra {
            ctxm.insert(k, v);
        }
    }
    m.insert(
        TermOrdKey(Term::Symbol(":error/context".to_string())),
        Term::Map(ctxm),
    );
    Value::Sealed {
        token,
        payload: Box::new(Value::Data(Term::Map(m))),
    }
}

pub(super) fn logged_resp(
    policy: &CapsPolicy,
    op: &str,
    store: &Option<ArtifactStore>,
    v: &Value,
    error_tok: SealId,
) -> Result<LoggedResp, EffectsError> {
    let resp = logged_resp_inline(v, error_tok)?;
    externalize_resp(policy, op, store.as_ref(), resp)
}

pub(super) fn logged_resp_inline(v: &Value, error_tok: SealId) -> Result<LoggedResp, EffectsError> {
    match v {
        Value::Data(t) => Ok(LoggedResp::Ok(t.clone())),
        Value::Sealed { token, payload } if *token == error_tok => {
            let Value::Data(t) = payload.as_ref() else {
                return Err(EffectsError::Log(
                    "sealed ERROR payload must be a datum for logging".to_string(),
                ));
            };
            Ok(LoggedResp::Error(t.clone()))
        }
        _ => Err(EffectsError::Log(format!(
            "response not serializable: {}",
            v.debug_repr()
        ))),
    }
}

pub(super) fn externalize_resp(
    policy: &CapsPolicy,
    op: &str,
    store: Option<&ArtifactStore>,
    resp: LoggedResp,
) -> Result<LoggedResp, EffectsError> {
    let Some(max_inline) = policy.inline_max_bytes_for(op) else {
        return Ok(resp);
    };
    let Some(store) = store else {
        return Err(EffectsError::Log(
            "caps.toml sets log inline_max_bytes but no store_dir is configured".to_string(),
        ));
    };

    match resp {
        LoggedResp::Ok(Term::Bytes(b)) => {
            if b.len() <= max_inline {
                Ok(LoggedResp::Ok(Term::Bytes(b)))
            } else {
                let hex = store.put_bytes(&b)?;
                Ok(LoggedResp::OkBytesArtifact { artifact: hex })
            }
        }
        LoggedResp::Error(Term::Bytes(b)) => {
            if b.len() <= max_inline {
                Ok(LoggedResp::Error(Term::Bytes(b)))
            } else {
                let hex = store.put_bytes(&b)?;
                Ok(LoggedResp::ErrorBytesArtifact { artifact: hex })
            }
        }
        LoggedResp::Ok(t) => {
            let s = print_term(&t);
            if s.len() <= max_inline {
                Ok(LoggedResp::Ok(t))
            } else {
                let hex = store.put_bytes(s.as_bytes())?;
                Ok(LoggedResp::OkArtifact { artifact: hex })
            }
        }
        LoggedResp::Error(t) => {
            let s = print_term(&t);
            if s.len() <= max_inline {
                Ok(LoggedResp::Error(t))
            } else {
                let hex = store.put_bytes(s.as_bytes())?;
                Ok(LoggedResp::ErrorArtifact { artifact: hex })
            }
        }
        other => Ok(other),
    }
}

pub(super) fn resp_from_log(
    resp: &LoggedResp,
    store: Option<&ArtifactStore>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    match resp {
        LoggedResp::Ok(t) => Ok(Value::Data(t.clone())),
        LoggedResp::Error(payload) => Ok(Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::Data(payload.clone())),
        }),
        LoggedResp::OkArtifact { artifact } => {
            let store = store.ok_or_else(|| {
                EffectsError::ReplayMismatch("missing artifact store for :ok-artifact".to_string())
            })?;
            let bytes = store.get_bytes(artifact)?;
            let s = String::from_utf8(bytes).map_err(|_| {
                EffectsError::ReplayMismatch("artifact bytes are not utf-8 term".to_string())
            })?;
            let t = gc_coreform::parse_term(&s)
                .map_err(|e| EffectsError::ReplayMismatch(format!("bad artifact term: {e}")))?;
            Ok(Value::Data(t))
        }
        LoggedResp::ErrorArtifact { artifact } => {
            let store = store.ok_or_else(|| {
                EffectsError::ReplayMismatch(
                    "missing artifact store for :error-artifact".to_string(),
                )
            })?;
            let bytes = store.get_bytes(artifact)?;
            let s = String::from_utf8(bytes).map_err(|_| {
                EffectsError::ReplayMismatch("artifact bytes are not utf-8 term".to_string())
            })?;
            let t = gc_coreform::parse_term(&s)
                .map_err(|e| EffectsError::ReplayMismatch(format!("bad artifact term: {e}")))?;
            Ok(Value::Sealed {
                token: error_tok,
                payload: Box::new(Value::Data(t)),
            })
        }
        LoggedResp::OkBytesArtifact { artifact } => {
            let store = store.ok_or_else(|| {
                EffectsError::ReplayMismatch(
                    "missing artifact store for :ok-bytes-artifact".to_string(),
                )
            })?;
            let bytes = store.get_bytes(artifact)?;
            Ok(Value::Data(Term::Bytes(bytes.into())))
        }
        LoggedResp::ErrorBytesArtifact { artifact } => {
            let store = store.ok_or_else(|| {
                EffectsError::ReplayMismatch(
                    "missing artifact store for :error-bytes-artifact".to_string(),
                )
            })?;
            let bytes = store.get_bytes(artifact)?;
            Ok(Value::Sealed {
                token: error_tok,
                payload: Box::new(Value::Data(Term::Bytes(bytes.into()))),
            })
        }
    }
}

pub(super) fn cap_term(op: &str, pol: Option<&OpPolicy>) -> Result<Term, EffectsError> {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":op".to_string())),
        Term::Symbol(op.to_string()),
    );
    if let Some(pol) = pol {
        if pol.create_dirs {
            m.insert(
                TermOrdKey(Term::Symbol(":create-dirs".to_string())),
                Term::Bool(true),
            );
        }
        if let Some(ms) = pol.timeout_ms {
            m.insert(
                TermOrdKey(Term::Symbol(":timeout-ms".to_string())),
                Term::Int((ms as i64).into()),
            );
        }
        if let Some(n) = pol.log_inline_max_bytes {
            m.insert(
                TermOrdKey(Term::Symbol(":log-inline-max-bytes".to_string())),
                Term::Int((n as i64).into()),
            );
        }
    }
    Ok(Term::Map(m))
}

pub(super) fn op_extra_positive_usize(
    pol: Option<&OpPolicy>,
    key: &str,
) -> Result<Option<usize>, String> {
    let Some(pol) = pol else {
        return Ok(None);
    };
    let Some(v) = pol.extra.get(key) else {
        return Ok(None);
    };
    let n = v
        .as_integer()
        .ok_or_else(|| format!("{key} must be a positive integer"))?;
    if n <= 0 {
        return Err(format!("{key} must be > 0"));
    }
    usize::try_from(n)
        .map(Some)
        .map_err(|_| format!("{key} is too large for this platform (max {})", usize::MAX))
}

pub(super) fn effective_limit(configured: Option<usize>, hard_limit: usize) -> usize {
    match configured {
        Some(v) => v.min(hard_limit),
        None => hard_limit,
    }
}

pub(super) fn mk_resource_limit_error(
    error_tok: SealId,
    op: &str,
    subject: &str,
    observed: usize,
    limit: usize,
) -> Value {
    mk_error(
        error_tok,
        "core/caps/resource-limit",
        format!("{subject} exceeded configured limit ({observed} > {limit} bytes)"),
        Some(op),
    )
}

pub(super) fn dispatch_op_alias(op: &str) -> &str {
    match op {
        // Compute is canonical under gpu/compute::*.
        // Keep gfx/gpu compute names as compatibility aliases.
        "gfx/gpu::create-compute-pipeline" => "gpu/compute::create-compute-pipeline",
        "gfx/gpu::submit-compute-graph" => "gpu/compute::submit",
        _ => op,
    }
}

pub(super) fn consume_budget(used: &mut usize, incoming: usize) {
    *used = used.saturating_add(incoming);
}

pub(super) fn externalized_resp_bytes(
    policy: &CapsPolicy,
    op: &str,
    value: &Value,
    error_tok: SealId,
) -> Result<Option<usize>, EffectsError> {
    let resp = logged_resp_inline(value, error_tok)?;
    let Some(max_inline) = policy.inline_max_bytes_for(op) else {
        return Ok(None);
    };

    match resp {
        LoggedResp::Ok(Term::Bytes(bytes)) | LoggedResp::Error(Term::Bytes(bytes)) => {
            if bytes.len() > max_inline {
                Ok(Some(bytes.len()))
            } else {
                Ok(None)
            }
        }
        LoggedResp::Ok(t) | LoggedResp::Error(t) => {
            let rendered = print_term(&t);
            if rendered.len() > max_inline {
                Ok(Some(rendered.len()))
            } else {
                Ok(None)
            }
        }
        LoggedResp::OkArtifact { .. }
        | LoggedResp::ErrorArtifact { .. }
        | LoggedResp::OkBytesArtifact { .. }
        | LoggedResp::ErrorBytesArtifact { .. } => Ok(None),
    }
}

pub(super) fn enforce_log_artifact_budget(
    policy: &CapsPolicy,
    budget: &mut ArtifactBudgetState,
    op: &str,
    value: &Value,
    error_tok: SealId,
) -> Result<Option<Value>, EffectsError> {
    let Some(bytes) = externalized_resp_bytes(policy, op, value, error_tok)? else {
        return Ok(None);
    };

    if let Some(limit) = policy.log.max_artifact_bytes_per_run {
        let observed = budget.log_artifact_written_bytes.saturating_add(bytes);
        if observed > limit {
            return Ok(Some(mk_resource_limit_error(
                error_tok,
                op,
                "log artifact bytes",
                observed,
                limit,
            )));
        }
    }
    if let Some(limit) = policy.store.max_run_bytes {
        let observed = budget.store_written_bytes.saturating_add(bytes);
        if observed > limit {
            return Ok(Some(mk_resource_limit_error(
                error_tok,
                op,
                "store artifact bytes",
                observed,
                limit,
            )));
        }
    }

    consume_budget(&mut budget.log_artifact_written_bytes, bytes);
    consume_budget(&mut budget.store_written_bytes, bytes);
    Ok(None)
}

pub(super) fn store_put_with_budget(
    store: &ArtifactStore,
    bytes: &[u8],
    policy: &CapsPolicy,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
) -> Result<String, Value> {
    if let Some(limit) = policy.store.max_run_bytes {
        let observed = budget.store_written_bytes.saturating_add(bytes.len());
        if observed > limit {
            return Err(mk_resource_limit_error(
                error_tok,
                op,
                "store artifact bytes",
                observed,
                limit,
            ));
        }
        let hex = store
            .put_bytes(bytes)
            .map_err(|e| mk_error(error_tok, "core/store/io-error", e.to_string(), Some(op)))?;
        budget.store_written_bytes = observed;
        return Ok(hex);
    }

    let hex = store
        .put_bytes(bytes)
        .map_err(|e| mk_error(error_tok, "core/store/io-error", e.to_string(), Some(op)))?;
    consume_budget(&mut budget.store_written_bytes, bytes.len());
    Ok(hex)
}
