use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use blake3::Hasher;
use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EffectProgram, EffectRequest, EvalCtx, SealId, Value, value_hash};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::error::EffectsError;
use crate::log::{Decision, EffectLog, EffectLogEntry, LoggedResp};
use crate::policy::{CapsPolicy, OpPolicy};
use crate::refs::{RefsDb, SetResult};
use crate::store::ArtifactStore;

struct HashingWriter<'a, W: std::io::Write> {
    inner: &'a mut W,
    hasher: blake3::Hasher,
}

impl<'a, W: std::io::Write> HashingWriter<'a, W> {
    fn new(inner: &'a mut W) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"GCv0.2\0gpk\0");
        Self { inner, hasher }
    }

    fn finish_hex(self) -> String {
        self.hasher.finalize().to_hex().to_string()
    }
}

impl<'a, W: std::io::Write> std::io::Write for HashingWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(buf);
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub struct RunResult {
    pub value: Value,
    pub log: EffectLog,
}

pub fn run(
    ctx: &mut EvalCtx,
    policy: &CapsPolicy,
    program: Value,
    program_hash: [u8; 32],
    toolchain: String,
) -> Result<RunResult, EffectsError> {
    let proto = ctx.protocol.ok_or(EffectsError::MissingProtocol)?;

    let store = match policy.artifact_store_dir() {
        Some(sd) => Some(ArtifactStore::open(sd)?),
        None => None,
    };

    let refs = match policy.refs_db_path() {
        Some(p) => Some(RefsDb::open(p)?),
        None => None,
    };

    let mut entries = Vec::new();
    let mut i: u64 = 0;
    let mut cur = program;

    loop {
        let Value::EffectProgram(p) = cur else {
            return Err(EffectsError::NotAnEffectProgram);
        };
        match p.as_ref() {
            EffectProgram::Pure(v) => {
                let log = EffectLog {
                    version: 2,
                    program_hash,
                    toolchain,
                    entries,
                };
                return Ok(RunResult {
                    value: (*v.as_ref()).clone(),
                    log,
                });
            }
            EffectProgram::Perform { request } => {
                let (req, sealed_token) = unseal_effect_request(request.as_ref(), proto.effect)?;
                if sealed_token != proto.effect {
                    return Err(EffectsError::BadEffectSeal);
                }

                let payload_h = hash_term(&req.payload);
                let cont_h = value_hash(&req.k);
                let req_h = hash_request(&req.op, payload_h, cont_h);

                let (decision, cap_term, resp_val, resp_logged) = if !policy.is_allowed(&req.op) {
                    let resp = mk_caps_denied(proto.error, &req.op);
                    (
                        Decision::Deny,
                        Term::Nil,
                        resp.clone(),
                        logged_resp(policy, &req.op, &store, &resp, proto.error)?,
                    )
                } else {
                    let pol = policy.op_policy(&req.op);
                    let cap_term = cap_term(&req.op, pol)?;
                    let resp = call_capability(
                        &req.op,
                        &req.payload,
                        pol,
                        store.as_ref(),
                        refs.as_ref(),
                        proto.error,
                    )?;
                    (
                        Decision::Allow,
                        cap_term,
                        resp.clone(),
                        logged_resp(policy, &req.op, &store, &resp, proto.error)?,
                    )
                };

                let resp_h = value_hash(&resp_val);

                entries.push(EffectLogEntry {
                    i,
                    op: req.op.clone(),
                    payload_h,
                    cont_h,
                    req_h,
                    decision,
                    cap: cap_term,
                    resp: resp_logged,
                    resp_h,
                });
                i = i.saturating_add(1);

                // Apply continuation; allow auto-lifting a non-effect-program result into Pure.
                let k = (*req.k).clone();
                let next = k.apply(ctx, resp_val)?;
                cur = match next {
                    Value::EffectProgram(_) => next,
                    other => Value::EffectProgram(Box::new(EffectProgram::Pure(Box::new(other)))),
                };
            }
        }
    }
}

pub fn replay(ctx: &mut EvalCtx, program: Value, log: &EffectLog) -> Result<Value, EffectsError> {
    replay_with_store(ctx, program, log, None)
}

pub fn replay_with_store(
    ctx: &mut EvalCtx,
    program: Value,
    log: &EffectLog,
    store: Option<&ArtifactStore>,
) -> Result<Value, EffectsError> {
    let proto = ctx.protocol.ok_or(EffectsError::MissingProtocol)?;
    let mut cur = program;
    let mut idx: usize = 0;
    loop {
        let Value::EffectProgram(p) = cur else {
            return Err(EffectsError::NotAnEffectProgram);
        };
        match p.as_ref() {
            EffectProgram::Pure(v) => {
                if idx != log.entries.len() {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "program finished with {} remaining log entries",
                        log.entries.len().saturating_sub(idx)
                    )));
                }
                return Ok((*v.as_ref()).clone());
            }
            EffectProgram::Perform { request } => {
                let entry = log.entries.get(idx).ok_or_else(|| {
                    EffectsError::ReplayMismatch("log ended before program finished".to_string())
                })?;
                let (req, sealed_token) = unseal_effect_request(request.as_ref(), proto.effect)?;
                if sealed_token != proto.effect {
                    return Err(EffectsError::BadEffectSeal);
                }

                let payload_h = hash_term(&req.payload);
                let cont_h = value_hash(&req.k);
                let req_h = hash_request(&req.op, payload_h, cont_h);

                if entry.i != idx as u64 {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "entry index mismatch: expected {}, got {}",
                        idx, entry.i
                    )));
                }
                if entry.op != req.op {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "op mismatch at {idx}: expected {}, got {}",
                        req.op, entry.op
                    )));
                }
                if entry.payload_h != payload_h {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "payload hash mismatch at {idx}"
                    )));
                }
                if entry.cont_h != cont_h {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "continuation hash mismatch at {idx}"
                    )));
                }
                if entry.req_h != req_h {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "request hash mismatch at {idx}"
                    )));
                }

                let resp_val = resp_from_log(&entry.resp, store, proto.error)?;

                let resp_h = value_hash(&resp_val);
                if entry.resp_h != resp_h {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "response hash mismatch at {idx}"
                    )));
                }

                let k = (*req.k).clone();
                let next = k.apply(ctx, resp_val)?;
                cur = match next {
                    Value::EffectProgram(_) => next,
                    other => Value::EffectProgram(Box::new(EffectProgram::Pure(Box::new(other)))),
                };

                idx += 1;
            }
        }
    }
}

fn unseal_effect_request(
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

fn hash_request(op: &str, payload_h: [u8; 32], cont_h: [u8; 32]) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(b"GCv0.2\0effect-req\0");
    h.update(op.as_bytes());
    h.update(b"\0");
    h.update(&payload_h);
    h.update(&cont_h);
    *h.finalize().as_bytes()
}

fn mk_caps_denied(error_tok: SealId, op: &str) -> Value {
    mk_error(
        error_tok,
        "core/caps/denied",
        format!("capability denied: {op}"),
        Some(op),
    )
}

fn mk_error(error_tok: SealId, code: &str, msg: String, op: Option<&str>) -> Value {
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

fn mk_error_with_ctx(
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

fn logged_resp(
    policy: &CapsPolicy,
    op: &str,
    store: &Option<ArtifactStore>,
    v: &Value,
    error_tok: SealId,
) -> Result<LoggedResp, EffectsError> {
    let resp = logged_resp_inline(v, error_tok)?;
    externalize_resp(policy, op, store.as_ref(), resp)
}

fn logged_resp_inline(v: &Value, error_tok: SealId) -> Result<LoggedResp, EffectsError> {
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

fn externalize_resp(
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

fn resp_from_log(
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
            Ok(Value::Data(Term::Bytes(bytes)))
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
                payload: Box::new(Value::Data(Term::Bytes(bytes))),
            })
        }
    }
}

fn cap_term(op: &str, pol: Option<&OpPolicy>) -> Result<Term, EffectsError> {
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

fn call_capability(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let timeout_ms = pol.and_then(|p| p.timeout_ms).filter(|ms| *ms > 0);
    if timeout_ms.is_some() && op == "io/fs::write" {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            "timeout_ms is not supported for io/fs::write (mutating op)".to_string(),
            Some(op),
        ));
    }
    match op {
        "core/sync::pull" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/sync::pull".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/sync::pull".to_string())
            })?;

            let remote_s = match payload_sync_remote(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op))),
            };
            let depth = payload_sync_depth(payload).unwrap_or(0);
            let force = payload_sync_force(payload).unwrap_or(false);
            let refnames = match payload_sync_refs(payload) {
                Ok(rs) => rs,
                Err(e) => return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op))),
            };
            let roots = match payload_sync_roots(payload) {
                Ok(rs) => rs,
                Err(e) => return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op))),
            };
            if refnames.is_empty() && roots.is_empty() {
                return Ok(mk_error(
                    error_tok,
                    "core/sync/bad-payload",
                    "pull requires :refs and/or :roots".to_string(),
                    Some(op),
                ));
            }

            let sp = match sync_policy_from_op(pol) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            let base = match sync_normalize_and_check_remote(&sp, &remote_s) {
                Ok(b) => b,
                Err(e) => return Ok(mk_error(error_tok, "core/sync/remote-denied", e, Some(op))),
            };
            let client = match gc_registry::RegistryClient::new(
                &base,
                timeout_ms.map(std::time::Duration::from_millis),
            ) {
                Ok(c) => c,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/sync/remote-error",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut pulled: u64 = 0;
            let mut already: u64 = 0;
            let mut heads: Vec<Term> = Vec::new();

            // Pull by roots (hashes) first.
            for h in &roots {
                let mut stats = SyncPullStats {
                    pulled: &mut pulled,
                    already: &mut already,
                    error_tok,
                    op,
                };
                match sync_pull_closure(&client, store, h, depth, &mut stats) {
                    Ok(()) => {}
                    Err(v) => return Ok(v),
                }
            }

            // Pull and update refs.
            for rname in &refnames {
                let h = match client.refs_get(rname) {
                    Ok(Some(h)) => h,
                    Ok(None) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/sync/ref-not-found",
                            format!("remote ref not found: {rname}"),
                            Some(op),
                        ));
                    }
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/sync/remote-error",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };
                let mut stats = SyncPullStats {
                    pulled: &mut pulled,
                    already: &mut already,
                    error_tok,
                    op,
                };
                match sync_pull_closure(&client, store, &h, depth, &mut stats) {
                    Ok(()) => {}
                    Err(v) => return Ok(v),
                }

                // Update local refs unless it would clobber a different value.
                let cur = refs.get(rname)?;
                if !force
                    && let Some(curh) = &cur
                    && curh != &h
                {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/refs/conflict",
                            "local ref differs; use force to overwrite".to_string(),
                            Some(op),
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":refs/name")),
                                        Term::Str(rname.clone()),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":refs/current")),
                                        cur.clone().map(Term::Str).unwrap_or(Term::Nil),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":refs/remote")),
                                        Term::Str(h.clone()),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                }
                let _ = refs.set(rname, Some(&h), None)?;

                heads.push(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":name")), Term::Str(rname.clone())),
                        (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }

            heads.sort_by_cached_key(print_term);

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(base));
            m.insert(
                TermOrdKey(Term::symbol(":pulled")),
                Term::Int((pulled as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":present")),
                Term::Int((already as i64).into()),
            );
            m.insert(TermOrdKey(Term::symbol(":heads")), Term::Vector(heads));
            Ok(Value::Data(Term::Map(m)))
        }

        "core/sync::push" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/sync::push".to_string())
            })?;

            let remote_s = match payload_sync_remote(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op))),
            };
            let depth = payload_sync_depth(payload).unwrap_or(0);
            let roots = match payload_sync_roots(payload) {
                Ok(rs) => rs,
                Err(e) => return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op))),
            };
            if roots.is_empty() {
                return Ok(mk_error(
                    error_tok,
                    "core/sync/bad-payload",
                    "push requires :roots".to_string(),
                    Some(op),
                ));
            }
            let set_refs = match payload_sync_set_refs(payload) {
                Ok(v) => v,
                Err(e) => return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op))),
            };

            let sp = match sync_policy_from_op(pol) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            let base = match sync_normalize_and_check_remote(&sp, &remote_s) {
                Ok(b) => b,
                Err(e) => return Ok(mk_error(error_tok, "core/sync/remote-denied", e, Some(op))),
            };
            let client = match gc_registry::RegistryClient::new(
                &base,
                timeout_ms.map(std::time::Duration::from_millis),
            ) {
                Ok(c) => c,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/sync/remote-error",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for h in &roots {
                match sync_closure_local(store, h, depth, &mut all, error_tok, op) {
                    Ok(()) => {}
                    Err(v) => return Ok(v),
                }
            }
            let hashes: Vec<String> = all.into_iter().collect();

            let mut missing: Vec<String> = Vec::new();
            let mut present: u64 = 0;
            for chunk in hashes.chunks(512) {
                let chunk_vec: Vec<String> = chunk.to_vec();
                let mp = match client.store_has(&chunk_vec) {
                    Ok(m) => m,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/sync/remote-error",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };
                for h in chunk {
                    match mp.get(h) {
                        Some(true) => present = present.saturating_add(1),
                        _ => missing.push(h.clone()),
                    }
                }
            }
            missing.sort();
            missing.dedup();

            let mut uploaded: u64 = 0;
            for h in &missing {
                let bytes = match store.get_bytes(h) {
                    Ok(b) => b,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-found",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };
                match client.store_put(h, &bytes) {
                    Ok(()) => uploaded = uploaded.saturating_add(1),
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/sync/remote-error",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                }
            }

            let mut refs_updated: u64 = 0;
            if !set_refs.is_empty() {
                let mut set_refs_sorted = set_refs;
                set_refs_sorted.sort_by(|a, b| a.name.cmp(&b.name));
                for sr in &set_refs_sorted {
                    let req = gc_registry::RefsSetReq {
                        name: &sr.name,
                        hash: &sr.hash,
                        policy: &sr.policy,
                        expected_old: sr.expected_old.as_deref(),
                    };
                    match client.refs_set(&req) {
                        Ok(r) => {
                            if !r.ok {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/sync/refs-set-failed",
                                    "remote refs/set returned ok=false".to_string(),
                                    Some(op),
                                ));
                            }
                            refs_updated = refs_updated.saturating_add(1);
                        }
                        Err(e) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/sync/remote-error",
                                format!("{e}"),
                                Some(op),
                            ));
                        }
                    }
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(base));
            m.insert(
                TermOrdKey(Term::symbol(":total")),
                Term::Int((hashes.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":present")),
                Term::Int((present as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":uploaded")),
                Term::Int((uploaded as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":refs-updated")),
                Term::Int((refs_updated as i64).into()),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::init" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let workspace = match payload_pkg_workspace(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let policy_s =
                payload_pkg_policy(payload).unwrap_or_else(|| "policy:default-v0.1".to_string());
            let reg_default = payload_pkg_registry_default(payload);

            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let lock_path = match sandbox_path_write(&base_dir, &lock_s, create_dirs) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut l = gc_pkg::GenesisLock::empty(workspace);
            l.policy = policy_s;
            if let Some(rd) = reg_default {
                l.registries.insert("default".to_string(), rd);
            }
            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            if let Err(e) = atomic_write_text(&lock_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h));
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::add" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let name = match payload_pkg_name(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let selector = match payload_pkg_selector(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let update_policy = match payload_pkg_update_policy(payload) {
                Ok(Some(p)) => p,
                Ok(None) => gc_pkg::UpdatePolicy::Manual,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let registry = payload_pkg_registry(payload);

            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let mut l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            l.set_requirement(&name, &selector, update_policy, registry);
            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let lock_write_path = match sandbox_path_write(&base_dir, &lock_s, false) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if let Err(e) = atomic_write_text(&lock_write_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h));
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::list" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut reqs = Vec::new();
            for (name, r) in &l.requirements {
                let mut mm = BTreeMap::new();
                mm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
                mm.insert(
                    TermOrdKey(Term::symbol(":selector")),
                    Term::Str(r.selector.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":update-policy")),
                    Term::Symbol(match r.update_policy {
                        gc_pkg::UpdatePolicy::Manual => ":manual".to_string(),
                        gc_pkg::UpdatePolicy::Auto => ":auto".to_string(),
                    }),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":registry")),
                    r.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                reqs.push(Term::Map(mm));
            }

            let mut locks = Vec::new();
            for (name, le) in &l.locked {
                let mut mm = BTreeMap::new();
                mm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
                mm.insert(
                    TermOrdKey(Term::symbol(":commit")),
                    le.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":snapshot")),
                    Term::Str(le.snapshot.clone()),
                );
                locks.push(Term::Map(mm));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":requirements")),
                Term::Vector(reqs),
            );
            m.insert(TermOrdKey(Term::symbol(":locked")), Term::Vector(locks));
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::info" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let name = match payload_pkg_name(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
            m.insert(
                TermOrdKey(Term::symbol(":requirement")),
                l.requirements
                    .get(&name)
                    .map(|r| {
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":selector")),
                                    Term::Str(r.selector.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":update-policy")),
                                    Term::Symbol(match r.update_policy {
                                        gc_pkg::UpdatePolicy::Manual => ":manual".to_string(),
                                        gc_pkg::UpdatePolicy::Auto => ":auto".to_string(),
                                    }),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":registry")),
                                    r.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .unwrap_or(Term::Nil),
            );
            m.insert(
                TermOrdKey(Term::symbol(":locked")),
                l.locked
                    .get(&name)
                    .map(|le| {
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":commit")),
                                    le.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":snapshot")),
                                    Term::Str(le.snapshot.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":resolved-ref")),
                                    le.resolved_ref.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .unwrap_or(Term::Nil),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::lock" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg::lock".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg::lock".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let mut l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut out_locked: BTreeMap<String, gc_pkg::LockedEntry> = BTreeMap::new();
            for (name, req) in &l.requirements {
                match resolve_requirement(store, refs, name, req, error_tok, op) {
                    Ok(le) => {
                        out_locked.insert(name.clone(), le);
                    }
                    Err(err_val) => return Ok(err_val),
                }
            }
            l.locked = out_locked;

            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let lock_write_path = match sandbox_path_write(&base_dir, &lock_s, false) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if let Err(e) = atomic_write_text(&lock_write_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h));
            m.insert(
                TermOrdKey(Term::symbol(":locked-count")),
                Term::Int((l.locked.len() as i64).into()),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::update" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg::update".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg::update".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let mut l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut updated: u64 = 0;
            for (name, req) in &l.requirements {
                let sel = parse_selector(&req.selector);
                let should_update = req.update_policy == gc_pkg::UpdatePolicy::Auto
                    && matches!(sel, Some(Selector::Ref(_)));
                if !should_update && l.locked.contains_key(name) {
                    continue;
                }
                match resolve_requirement(store, refs, name, req, error_tok, op) {
                    Ok(le) => {
                        l.locked.insert(name.clone(), le);
                        updated = updated.saturating_add(1);
                    }
                    Err(err_val) => return Ok(err_val),
                }
            }

            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let lock_write_path = match sandbox_path_write(&base_dir, &lock_s, false) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if let Err(e) = atomic_write_text(&lock_write_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h));
            m.insert(
                TermOrdKey(Term::symbol(":updated")),
                Term::Int((updated as i64).into()),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::install" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg::install".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let frozen = payload_pkg_bool(payload, ":frozen").unwrap_or(false);
            let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);

            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if frozen {
                let missing = l.requirements_missing_locks();
                if !missing.is_empty() {
                    return Ok(mk_error_with_ctx(
                        error_tok,
                        "core/pkg/not-locked",
                        "lock is missing locked entries".to_string(),
                        Some(op),
                        Term::Map(
                            [(
                                TermOrdKey(Term::symbol(":missing")),
                                Term::Vector(missing.into_iter().map(Term::Str).collect()),
                            )]
                            .into_iter()
                            .collect(),
                        ),
                    ));
                }
            }

            let mut ok = true;
            let mut missing_hashes: Vec<Term> = Vec::new();
            let mut checked: u64 = 0;

            for (name, le) in &l.locked {
                // Snapshot must exist and be well-formed.
                let snapshot_hex = &le.snapshot;
                if !store.path_for(snapshot_hex).exists() {
                    ok = false;
                    missing_hashes.push(Term::Str(snapshot_hex.clone()));
                    continue;
                }
                if store.verify_hex(snapshot_hex).is_err() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {snapshot_hex}"),
                        Some(op),
                    ));
                }
                let snap_term = match store_get_term(store, snapshot_hex) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let snap = match gc_vcs::Snapshot::from_term(&snap_term) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };

                // Shallow closure: snapshot + module artifacts.
                let mut hashes: Vec<String> = Vec::new();
                hashes.push(snapshot_hex.clone());
                hashes.extend(snap.shallow_refs());
                hashes.sort();
                hashes.dedup();

                for h in hashes {
                    if !store.path_for(&h).exists() {
                        ok = false;
                        missing_hashes.push(Term::Str(h));
                        continue;
                    }
                    if store.verify_hex(&h).is_err() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("artifact store corruption: {h}"),
                            Some(op),
                        ));
                    }
                    checked = checked.saturating_add(1);
                }

                if strict && let Some(commit_hex) = &le.commit {
                    let commit_term = match store_get_term(store, commit_hex) {
                        Ok(t) => t,
                        Err(_) => {
                            return Ok(mk_error(
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
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-commit",
                                e.to_string(),
                                Some(op),
                            ));
                        }
                    };
                    if c.result != *snapshot_hex {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/commit-snapshot-mismatch",
                            format!("commit.result != locked.snapshot for {name}"),
                            Some(op),
                        ));
                    }
                    for evh in &c.evidence {
                        if !store.path_for(evh).exists() {
                            ok = false;
                            missing_hashes.push(Term::Str(evh.clone()));
                            continue;
                        }
                        if store.verify_hex(evh).is_err() {
                            return Ok(mk_error(
                                error_tok,
                                "core/store/corruption",
                                format!("artifact store corruption: {evh}"),
                                Some(op),
                            ));
                        }
                        let ev_term = match store_get_term(store, evh) {
                            Ok(t) => t,
                            Err(e) => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/pkg/bad-evidence",
                                    e.to_string(),
                                    Some(op),
                                ));
                            }
                        };
                        if let Err(e) = gc_vcs::Evidence::from_term(&ev_term) {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-evidence",
                                e.to_string(),
                                Some(op),
                            ));
                        }
                        checked = checked.saturating_add(1);
                    }
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":checked")),
                Term::Int((checked as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":missing")),
                Term::Vector(missing_hashes),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::verify" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg::verify".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };

            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut ok = true;
            let mut missing_hashes: Vec<Term> = Vec::new();
            let mut checked: u64 = 0;

            for (name, le) in &l.locked {
                let snapshot_hex = &le.snapshot;
                if !store.path_for(snapshot_hex).exists() {
                    ok = false;
                    missing_hashes.push(Term::Str(snapshot_hex.clone()));
                    continue;
                }
                if store.verify_hex(snapshot_hex).is_err() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {snapshot_hex}"),
                        Some(op),
                    ));
                }
                let snap_term = match store_get_term(store, snapshot_hex) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let snap = match gc_vcs::Snapshot::from_term(&snap_term) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let mut hashes: Vec<String> = Vec::new();
                hashes.push(snapshot_hex.clone());
                hashes.extend(snap.shallow_refs());
                hashes.sort();
                hashes.dedup();
                for h in hashes {
                    if !store.path_for(&h).exists() {
                        ok = false;
                        missing_hashes.push(Term::Str(h));
                        continue;
                    }
                    if store.verify_hex(&h).is_err() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("artifact store corruption: {h}"),
                            Some(op),
                        ));
                    }
                    checked = checked.saturating_add(1);
                }

                if let Some(commit_hex) = &le.commit {
                    let commit_term = match store_get_term(store, commit_hex) {
                        Ok(t) => t,
                        Err(_) => {
                            return Ok(mk_error(
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
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-commit",
                                e.to_string(),
                                Some(op),
                            ));
                        }
                    };
                    if c.result != *snapshot_hex {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/commit-snapshot-mismatch",
                            format!("commit.result != locked.snapshot for {name}"),
                            Some(op),
                        ));
                    }
                    for evh in &c.evidence {
                        if !store.path_for(evh).exists() {
                            ok = false;
                            missing_hashes.push(Term::Str(evh.clone()));
                            continue;
                        }
                        if store.verify_hex(evh).is_err() {
                            return Ok(mk_error(
                                error_tok,
                                "core/store/corruption",
                                format!("artifact store corruption: {evh}"),
                                Some(op),
                            ));
                        }
                        let ev_term = match store_get_term(store, evh) {
                            Ok(t) => t,
                            Err(e) => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/pkg/bad-evidence",
                                    e.to_string(),
                                    Some(op),
                                ));
                            }
                        };
                        if let Err(e) = gc_vcs::Evidence::from_term(&ev_term) {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-evidence",
                                e.to_string(),
                                Some(op),
                            ));
                        }
                        checked = checked.saturating_add(1);
                    }
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":checked")),
                Term::Int((checked as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":missing")),
                Term::Vector(missing_hashes),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::snapshot" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg::snapshot".to_string())
            })?;
            let pkg_path_s = match payload_pkg_path(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let base_dir = effective_base_dir(pol)?;
            let pkg_path = sandbox_path_read(&base_dir, &pkg_path_s)?;

            let (manifest, pkg_dir) = match gc_pkg::PackageManifest::load(&pkg_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/manifest-error",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            // Ensure the package directory is within the sandbox base.
            let base = std::fs::canonicalize(&base_dir)?;
            let pkg_dir_resolved = std::fs::canonicalize(&pkg_dir)?;
            if !pkg_dir_resolved.starts_with(&base) {
                return Ok(mk_error(
                    error_tok,
                    "core/caps/path-escape",
                    "package directory escapes base dir".to_string(),
                    Some(op),
                ));
            }

            let mut modules_out: Vec<Term> = Vec::new();
            for me in &manifest.modules {
                let module_fs_path = pkg_dir.join(&me.path);
                let resolved = match std::fs::canonicalize(&module_fs_path) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/pkg/io-error",
                            e.to_string(),
                            Some(op),
                            Term::Map(
                                [(
                                    TermOrdKey(Term::symbol(":path")),
                                    Term::Str(me.path.clone()),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                };
                if !resolved.starts_with(&base) {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("module escapes base dir: {}", me.path),
                        Some(op),
                    ));
                }
                let src = match std::fs::read_to_string(&resolved) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/pkg/io-error",
                            e.to_string(),
                            Some(op),
                            Term::Map(
                                [(
                                    TermOrdKey(Term::symbol(":path")),
                                    Term::Str(me.path.clone()),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                };
                let forms = match gc_coreform::parse_module(&src) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/parse-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let forms = match gc_coreform::canonicalize_module(forms) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/canon-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let module_h = gc_coreform::hash_module(&forms);
                if let Some(want_hex) = &me.hash {
                    if want_hex.len() != 64 || !want_hex.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-hash",
                            format!("manifest module hash is not 64-hex: {}", me.path),
                            Some(op),
                        ));
                    }
                    let got_hex = blake3::Hash::from_bytes(module_h).to_hex().to_string();
                    if &got_hex != want_hex {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/hash-mismatch",
                            format!("module hash mismatch: {}", me.path),
                            Some(op),
                        ));
                    }
                }

                let module_art = Term::Vector(forms);
                let module_bytes = print_term(&module_art);
                let store_hex = match store.put_bytes(module_bytes.as_bytes()) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let mut mm = BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::Symbol(":path".to_string())),
                    Term::Str(me.path.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::Symbol(":hash".to_string())),
                    Term::Str(store_hex),
                );
                mm.insert(
                    TermOrdKey(Term::Symbol(":module-h".to_string())),
                    Term::Bytes(module_h.to_vec()),
                );
                modules_out.push(Term::Map(mm));
            }

            let snapshot = Term::Map(
                [
                    (
                        TermOrdKey(Term::Symbol(":type".to_string())),
                        Term::Symbol(":vcs/snapshot".to_string()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":v".to_string())),
                        Term::Int(1.into()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":kind".to_string())),
                        Term::Symbol(":package".to_string()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":pkg/name".to_string())),
                        Term::Str(manifest.name.clone()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":pkg/version".to_string())),
                        Term::Str(manifest.version.clone()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":modules".to_string())),
                        Term::Vector(modules_out.clone()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":obligations".to_string())),
                        Term::Vector(
                            manifest
                                .obligations
                                .iter()
                                .cloned()
                                .map(Term::Symbol)
                                .collect(),
                        ),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let snap_hex = match store.put_bytes(print_term(&snapshot).as_bytes()) {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            let mut out = BTreeMap::new();
            out.insert(
                TermOrdKey(Term::Symbol(":snapshot".to_string())),
                Term::Str(snap_hex),
            );
            out.insert(
                TermOrdKey(Term::Symbol(":modules".to_string())),
                Term::Vector(modules_out),
            );
            Ok(Value::Data(Term::Map(out)))
        }
        "core/store::put" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/store::put".to_string())
            })?;
            let art = payload_store_artifact(payload)?;
            let bytes = print_term(&art);
            let h = store.put_bytes(bytes.as_bytes())?;
            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::Symbol(":hash".to_string())), Term::Str(h));
            Ok(Value::Data(Term::Map(m)))
        }
        "core/store::has" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/store::has".to_string())
            })?;
            let h = payload_store_hash(payload)?;
            let p = store.path_for(&h);
            let present = if p.exists() {
                store.verify_hex(&h)?;
                true
            } else {
                false
            };
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":present".to_string())),
                Term::Bool(present),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/store::get" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/store::get".to_string())
            })?;
            let h = payload_store_hash(payload)?;
            let p = store.path_for(&h);
            if !p.exists() {
                return Ok(mk_error(
                    error_tok,
                    "core/store/not-found",
                    format!("artifact not found: {h}"),
                    Some(op),
                ));
            }
            let bytes = store.get_bytes(&h)?;
            let s = String::from_utf8(bytes)
                .map_err(|_| EffectsError::Log("artifact bytes are not utf-8 term".to_string()))?;
            let t = gc_coreform::parse_term(&s)
                .map_err(|e| EffectsError::Log(format!("bad artifact term: {e}")))?;
            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::Symbol(":artifact".to_string())), t);
            Ok(Value::Data(Term::Map(m)))
        }
        "core/gpk::export" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gpk::export".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/gpk::export".to_string())
            });
            let root_hex = match payload_gpk_root(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let out_path_s = match payload_gpk_out(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let out_path = sandbox_path_write(&base_dir, &out_path_s, create_dirs)?;

            let mode = match payload_gpk_mode(payload) {
                Ok(Some(m)) => m,
                Ok(None) => ":shallow".to_string(),
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let depth = payload_gpk_depth(payload).unwrap_or(0);
            let embed_refnames = match payload_gpk_refs(payload) {
                Ok(xs) => xs,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let root_term = match store_get_term(store, &root_hex) {
                Ok(t) => t,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {root_hex}"),
                        Some(op),
                    ));
                }
            };

            let hashes: Vec<String> = if mode == ":shallow" {
                // Root must be a snapshot.
                let snap = match gc_vcs::Snapshot::from_term(&root_term) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/gpk/bad-root",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };
                let mut hs: Vec<String> = Vec::new();
                hs.push(root_hex.clone());
                hs.extend(snap.shallow_refs());
                hs.sort();
                hs.dedup();
                hs
            } else if mode == ":full" {
                let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
                match sync_closure_local(store, &root_hex, depth, &mut all, error_tok, op) {
                    Ok(()) => {}
                    Err(v) => return Ok(v),
                }
                all.into_iter().collect()
            } else {
                return Ok(mk_error(
                    error_tok,
                    "core/gpk/bad-payload",
                    format!("unsupported :mode {mode}"),
                    Some(op),
                ));
            };

            let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
            for h in &hashes {
                if !store.path_for(h).exists() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {h}"),
                        Some(op),
                    ));
                }
                if store.verify_hex(h).is_err() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {h}"),
                        Some(op),
                    ));
                }
                let bytes = match store.get_bytes(h) {
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
                entries.push((h.clone(), bytes));
            }

            let root_b = match gc_vcs::hex_to_bytes32(&root_hex) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/gpk/bad-root", e, Some(op)));
                }
            };

            let mut refs_section: Vec<(String, String)> = Vec::new();
            let bundle_version: u32 = if embed_refnames.is_empty() { 1 } else { 2 };
            if bundle_version == 2 {
                let refs = match refs {
                    Ok(r) => r,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/gpk/missing-refs-db",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                for name in &embed_refnames {
                    let cur = match refs.get(name) {
                        Ok(h) => h,
                        Err(e) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/gpk/refs-io-error",
                                e.to_string(),
                                Some(op),
                            ));
                        }
                    };
                    let Some(h) = cur else {
                        return Ok(mk_error(
                            error_tok,
                            "core/gpk/ref-not-found",
                            format!("ref not found: {name}"),
                            Some(op),
                        ));
                    };
                    refs_section.push((name.clone(), h));
                }
                refs_section.sort_by(|a, b| a.0.cmp(&b.0));
                refs_section.dedup_by(|a, b| a.0 == b.0);
            }

            let mut file = match std::fs::File::create(&out_path) {
                Ok(f) => f,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let bundle_h = {
                let mut hw = HashingWriter::new(&mut file);
                let refs_opt = if bundle_version == 2 {
                    Some(refs_section.as_slice())
                } else {
                    None
                };
                if let Err(e) = gc_vcs::write_bundle(&mut hw, bundle_version, root_b, &entries, refs_opt) {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/write-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
                hw.finish_hex()
            };
            if let Err(e) = file.sync_all() {
                return Ok(mk_error(
                    error_tok,
                    "core/gpk/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":ok".to_string())),
                Term::Bool(true),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":bundle-h".to_string())),
                Term::Str(bundle_h),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":bundle-v".to_string())),
                Term::Int((bundle_version as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":root".to_string())),
                Term::Str(root_hex),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":count".to_string())),
                Term::Int((hashes.len() as i64).into()),
            );
            if bundle_version == 2 {
                let out_refs: Vec<Term> = refs_section
                    .iter()
                    .map(|(n, h)| {
                        Term::Map(
                            [
                                (TermOrdKey(Term::symbol(":name")), Term::Str(n.clone())),
                                (TermOrdKey(Term::symbol(":hash")), Term::Str(h.clone())),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .collect();
                m.insert(TermOrdKey(Term::symbol(":refs")), Term::Vector(out_refs));
            }
            Ok(Value::Data(Term::Map(m)))
        }
        "core/gpk::import" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gpk::import".to_string())
            })?;
            let in_path_s = match payload_gpk_in(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let base_dir = effective_base_dir(pol)?;
            let in_path = sandbox_path_read(&base_dir, &in_path_s)?;
            let mut f = match std::fs::File::open(&in_path) {
                Ok(f) => f,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let bundle = match gc_vcs::read_bundle(&mut f) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/read-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let root_hex = gc_vcs::bytes32_to_hex(&bundle.root);

            for e in &bundle.entries {
                let expected = gc_vcs::bytes32_to_hex(&e.hash);
                let got = match store.put_bytes(&e.bytes) {
                    Ok(h) => h,
                    Err(err) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/io-error",
                            err.to_string(),
                            Some(op),
                        ));
                    }
                };
                if got != expected {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/hash-mismatch",
                        "bundle entry hash mismatch".to_string(),
                        Some(op),
                    ));
                }
            }

            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":ok".to_string())),
                Term::Bool(true),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":root".to_string())),
                Term::Str(root_hex),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":bundle-v".to_string())),
                Term::Int((bundle.version as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":count".to_string())),
                Term::Int((bundle.entries.len() as i64).into()),
            );
            if !bundle.refs.is_empty() {
                let mut rs: Vec<Term> = Vec::new();
                for rr in &bundle.refs {
                    rs.push(Term::Map(
                        [
                            (TermOrdKey(Term::symbol(":name")), Term::Str(rr.name.clone())),
                            (
                                TermOrdKey(Term::symbol(":hash")),
                                Term::Str(gc_vcs::bytes32_to_hex(&rr.hash)),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ));
                }
                m.insert(TermOrdKey(Term::symbol(":refs")), Term::Vector(rs));
            }
            Ok(Value::Data(Term::Map(m)))
        }
        "core/refs::get" => {
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/refs::get".to_string())
            })?;
            let name = payload_refs_name(payload)?;
            let h = refs.get(&name)?;
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":name".to_string())),
                Term::Str(name),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":hash".to_string())),
                h.map(Term::Str).unwrap_or(Term::Nil),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/refs::list" => {
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/refs::list".to_string())
            })?;
            let prefix = payload_refs_prefix(payload)?;
            let xs = refs.list(prefix.as_deref())?;
            let mut out = Vec::new();
            for e in xs {
                let mut m = BTreeMap::new();
                m.insert(
                    TermOrdKey(Term::Symbol(":name".to_string())),
                    Term::Str(e.name),
                );
                m.insert(
                    TermOrdKey(Term::Symbol(":hash".to_string())),
                    e.hash.map(Term::Str).unwrap_or(Term::Nil),
                );
                out.push(Term::Map(m));
            }
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":refs".to_string())),
                Term::Vector(out),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/refs::set" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/refs::set".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/refs::set".to_string())
            })?;

            let name = payload_refs_name(payload)?;
            let new_hash = payload_refs_hash(payload)?;
            let expected_old = payload_refs_expected_old(payload)?;
            let policy_h = payload_refs_policy_hash(payload)?;

            let pol_term = match store_get_term(store, &policy_h) {
                Ok(t) => t,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/refs/policy-not-found",
                        format!("policy artifact not found: {policy_h}"),
                        Some(op),
                    ));
                }
            };
            let pol = match gc_vcs::Policy::from_term(&pol_term) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/refs/bad-policy",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if pol.is_frozen_ref(&name) {
                return Ok(mk_error(
                    error_tok,
                    "core/refs/frozen",
                    format!("ref is frozen by policy: {name}"),
                    Some(op),
                ));
            }
            let class = match pol.class_for_ref(&name) {
                Some(c) => c,
                None => {
                    return Ok(mk_error(
                        error_tok,
                        "core/refs/no-class",
                        format!("policy has no matching class for ref {name}"),
                        Some(op),
                    ));
                }
            };

            if let Some(h) = &new_hash {
                let commit_term = match store_get_term(store, h) {
                    Ok(t) => t,
                    Err(_) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/refs/commit-not-found",
                            format!("commit artifact not found: {h}"),
                            Some(op),
                        ));
                    }
                };
                let commit = match gc_vcs::Commit::from_term(&commit_term) {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/refs/bad-commit",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };

                for req in &class.required_obligations {
                    if !commit.obligations.iter().any(|o| o == req) {
                        return Ok(mk_error(
                            error_tok,
                            "core/refs/missing-obligation",
                            format!("commit missing required obligation: {req}"),
                            Some(op),
                        ));
                    }
                }
                if !class.required_obligations.is_empty() && commit.evidence.is_empty() {
                    return Ok(mk_error(
                        error_tok,
                        "core/refs/missing-evidence",
                        "commit has required obligations but no evidence".to_string(),
                        Some(op),
                    ));
                }
                for ev_h in &commit.evidence {
                    if store.path_for(ev_h).exists() {
                        if store.verify_hex(ev_h).is_err() {
                            return Ok(mk_error(
                                error_tok,
                                "core/store/corruption",
                                format!("evidence artifact corrupted: {ev_h}"),
                                Some(op),
                            ));
                        }
                    } else {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-found",
                            format!("evidence artifact not found: {ev_h}"),
                            Some(op),
                        ));
                    }
                    let ev_t = match store_get_term(store, ev_h) {
                        Ok(t) => t,
                        Err(_) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/refs/bad-evidence",
                                format!("evidence artifact is not a valid CoreForm term: {ev_h}"),
                                Some(op),
                            ));
                        }
                    };
                    if let Err(e) = gc_vcs::Evidence::from_term(&ev_t) {
                        return Ok(mk_error(
                            error_tok,
                            "core/refs/bad-evidence",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                }

                if class.require_signatures {
                    let signing_h = match gc_vcs::commit_signing_hash(&commit_term) {
                        Ok(hh) => hh,
                        Err(e) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/refs/bad-commit",
                                format!("{e}"),
                                Some(op),
                            ));
                        }
                    };
                    let mut valid: u64 = 0;
                    let mut seen_pks: std::collections::BTreeSet<Vec<u8>> =
                        std::collections::BTreeSet::new();
                    for at_h in &commit.attestations {
                        if store.path_for(at_h).exists() {
                            if store.verify_hex(at_h).is_err() {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/store/corruption",
                                    format!("attestation artifact corrupted: {at_h}"),
                                    Some(op),
                                ));
                            }
                        } else {
                            return Ok(mk_error(
                                error_tok,
                                "core/store/not-found",
                                format!("attestation artifact not found: {at_h}"),
                                Some(op),
                            ));
                        }
                        let at_t = match store_get_term(store, at_h) {
                            Ok(t) => t,
                            Err(_) => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/refs/bad-attestation",
                                    format!(
                                        "attestation artifact is not a valid CoreForm term: {at_h}"
                                    ),
                                    Some(op),
                                ));
                            }
                        };
                        let at = match gc_vcs::Attestation::from_term(&at_t) {
                            Ok(a) => a,
                            Err(e) => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/refs/bad-attestation",
                                    format!("{e}"),
                                    Some(op),
                                ));
                            }
                        };
                        let pk_vec = at.pk.to_vec();
                        if seen_pks.contains(&pk_vec) {
                            continue;
                        }
                        if gc_vcs::verify_commit_attestation(
                            &at,
                            &signing_h,
                            &class.allowed_public_keys,
                        )
                        .is_ok()
                        {
                            seen_pks.insert(pk_vec);
                            valid = valid.saturating_add(1);
                        }
                    }
                    if valid < class.min_signatures {
                        return Ok(mk_error(
                            error_tok,
                            "core/refs/insufficient-signatures",
                            format!(
                                "need {} valid signatures, got {valid}",
                                class.min_signatures
                            ),
                            Some(op),
                        ));
                    }
                }
            }

            match refs.set(
                &name,
                new_hash.as_deref(),
                expected_old.as_ref().map(|x| x.as_deref()),
            )? {
                SetResult::Updated => {
                    let mut m = BTreeMap::new();
                    m.insert(
                        TermOrdKey(Term::Symbol(":ok".to_string())),
                        Term::Bool(true),
                    );
                    m.insert(
                        TermOrdKey(Term::Symbol(":name".to_string())),
                        Term::Str(name),
                    );
                    m.insert(
                        TermOrdKey(Term::Symbol(":hash".to_string())),
                        new_hash.map(Term::Str).unwrap_or(Term::Nil),
                    );
                    Ok(Value::Data(Term::Map(m)))
                }
                SetResult::Conflict { current } => Ok(mk_error_with_ctx(
                    error_tok,
                    "core/refs/conflict",
                    "ref update conflict".to_string(),
                    Some(op),
                    Term::Map(
                        [(
                            TermOrdKey(Term::Symbol(":refs/current".to_string())),
                            current.map(Term::Str).unwrap_or(Term::Nil),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                )),
            }
        }
        "core/refs::delete" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/refs::delete".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/refs::delete".to_string())
            })?;
            let name = payload_refs_name(payload)?;
            let expected_old = payload_refs_expected_old(payload)?;
            let policy_h = payload_refs_policy_hash(payload)?;
            let pol_term = match store_get_term(store, &policy_h) {
                Ok(t) => t,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/refs/policy-not-found",
                        format!("policy artifact not found: {policy_h}"),
                        Some(op),
                    ));
                }
            };
            let pol = match gc_vcs::Policy::from_term(&pol_term) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/refs/bad-policy",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if pol.is_frozen_ref(&name) {
                return Ok(mk_error(
                    error_tok,
                    "core/refs/frozen",
                    format!("ref is frozen by policy: {name}"),
                    Some(op),
                ));
            }
            if pol.class_for_ref(&name).is_none() {
                return Ok(mk_error(
                    error_tok,
                    "core/refs/no-class",
                    format!("policy has no matching class for ref {name}"),
                    Some(op),
                ));
            }

            match refs.set(&name, None, expected_old.as_ref().map(|x| x.as_deref()))? {
                SetResult::Updated => {
                    let mut m = BTreeMap::new();
                    m.insert(
                        TermOrdKey(Term::Symbol(":ok".to_string())),
                        Term::Bool(true),
                    );
                    m.insert(
                        TermOrdKey(Term::Symbol(":name".to_string())),
                        Term::Str(name),
                    );
                    Ok(Value::Data(Term::Map(m)))
                }
                SetResult::Conflict { current } => Ok(mk_error_with_ctx(
                    error_tok,
                    "core/refs/conflict",
                    "ref delete conflict".to_string(),
                    Some(op),
                    Term::Map(
                        [(
                            TermOrdKey(Term::Symbol(":refs/current".to_string())),
                            current.map(Term::Str).unwrap_or(Term::Nil),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                )),
            }
        }
        "sys/time::now" => {
            if let Some(ms) = timeout_ms {
                let r = with_timeout(ms, || {
                    Ok(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis())
                })?;
                return Ok(match r {
                    Some(t) => Value::Data(Term::Int(BigInt::from(t))),
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: sys/time::now"),
                        Some(op),
                    ),
                });
            }
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            Ok(Value::Data(Term::Int(BigInt::from(t))))
        }
        "io/fs::read" => {
            let path_s = payload_path(payload)?;
            let base_dir = effective_base_dir(pol)?;
            if let Some(ms) = timeout_ms {
                let base_dir2 = base_dir.clone();
                let path_s2 = path_s.clone();
                let r = with_timeout(ms, move || {
                    let path = sandbox_path_read(&base_dir2, &path_s2)?;
                    let bytes = std::fs::read(&path);
                    Ok((path, bytes))
                })?;
                return Ok(match r {
                    Some((_path, Ok(bytes))) => Value::Data(Term::Bytes(bytes)),
                    Some((path, Err(e))) => Value::Sealed {
                        token: error_tok,
                        payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                    },
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: io/fs::read"),
                        Some(op),
                    ),
                });
            }
            let path = sandbox_path_read(&base_dir, &path_s)?;
            match std::fs::read(&path) {
                Ok(bytes) => Ok(Value::Data(Term::Bytes(bytes))),
                Err(e) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                }),
            }
        }
        "io/fs::write" => {
            let path_s = payload_path(payload)?;
            let data = payload_data(payload)?;
            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.is_some_and(|p| p.create_dirs);
            let path = sandbox_path_write(&base_dir, &path_s, create_dirs)?;
            if path.exists() {
                let md = std::fs::symlink_metadata(&path)?;
                if md.file_type().is_symlink() {
                    let e = std::io::Error::other("refusing to write through symlink");
                    return Ok(Value::Sealed {
                        token: error_tok,
                        payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                    });
                }
            }
            match std::fs::write(&path, data) {
                Ok(()) => Ok(Value::Data(Term::Nil)),
                Err(e) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                }),
            }
        }
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op",
            format!("unknown capability op: {op}"),
            Some(op),
        )),
    }
}

fn payload_store_hash(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/store payload must be a map".to_string(),
        ));
    };
    let Some(Term::Str(h)) = m.get(&TermOrdKey(Term::Symbol(":hash".to_string()))) else {
        return Err(EffectsError::Log(
            "core/store payload missing :hash string".to_string(),
        ));
    };
    Ok(h.clone())
}

fn payload_store_artifact(payload: &Term) -> Result<Term, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/store payload must be a map".to_string(),
        ));
    };
    let Some(t) = m.get(&TermOrdKey(Term::Symbol(":artifact".to_string()))) else {
        return Err(EffectsError::Log(
            "core/store payload missing :artifact".to_string(),
        ));
    };
    Ok(t.clone())
}

fn store_get_term(store: &ArtifactStore, hex: &str) -> Result<Term, EffectsError> {
    let bytes = store.get_bytes(hex)?;
    let s = String::from_utf8(bytes)
        .map_err(|_| EffectsError::Log("artifact bytes are not utf-8 term".to_string()))?;
    gc_coreform::parse_term(&s).map_err(|e| EffectsError::Log(format!("bad artifact term: {e}")))
}

fn payload_refs_name(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":name".to_string()))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err(EffectsError::Log(
            "core/refs payload missing :name".to_string(),
        )),
    }
}

fn payload_refs_prefix(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":prefix".to_string()))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        _ => Err(EffectsError::Log(
            "core/refs payload :prefix must be string or nil".to_string(),
        )),
    }
}

fn payload_refs_hash(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":hash".to_string()))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        _ => Err(EffectsError::Log(
            "core/refs payload :hash must be string or nil".to_string(),
        )),
    }
}

fn payload_refs_expected_old(payload: &Term) -> Result<Option<Option<String>>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":expected-old".to_string()))) {
        None => Ok(None),
        Some(Term::Nil) => Ok(Some(None)),
        Some(Term::Str(s)) => Ok(Some(Some(s.clone()))),
        _ => Err(EffectsError::Log(
            "core/refs payload :expected-old must be string, nil, or absent".to_string(),
        )),
    }
}

fn payload_refs_policy_hash(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":policy".to_string()))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err(EffectsError::Log(
            "core/refs payload missing :policy".to_string(),
        )),
    }
}

fn with_timeout<T, F>(timeout_ms: u64, f: F) -> Result<Option<T>, EffectsError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, EffectsError> + Send + 'static,
{
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let r = f();
        let _ = tx.send(r);
    });
    match rx.recv_timeout(std::time::Duration::from_millis(timeout_ms)) {
        Ok(r) => r.map(Some),
        Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(EffectsError::Log(
            "capability thread disconnected".to_string(),
        )),
    }
}

fn io_error_payload(op: &str, base_dir: &Path, path: &Path, e: &std::io::Error) -> Term {
    // Avoid leaking absolute paths and normalize separators for stability.
    let rel = path.strip_prefix(base_dir).unwrap_or(path);
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":error/code".to_string())),
        Term::Str("io/error".to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/message".to_string())),
        Term::Str(e.to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/op".to_string())),
        Term::Symbol(op.to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/context".to_string())),
        Term::Map(
            [
                (
                    TermOrdKey(Term::Symbol(":subsystem".to_string())),
                    Term::Str("effects".to_string()),
                ),
                (
                    TermOrdKey(Term::Symbol(":op".to_string())),
                    Term::Symbol(op.to_string()),
                ),
                (
                    TermOrdKey(Term::Symbol(":path".to_string())),
                    Term::Str(path_to_slash(rel)),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    );
    Term::Map(m)
}

fn path_to_slash(p: &Path) -> String {
    let mut out = String::new();
    for (i, c) in p.components().enumerate() {
        if i != 0 {
            out.push('/');
        }
        out.push_str(&c.as_os_str().to_string_lossy());
    }
    out
}

fn payload_path(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":path".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :path".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":path must be string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_pkg_path(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":pkg".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :pkg".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":pkg must be string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_gpk_root(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":root".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :root".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":root must be string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_gpk_out(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":out".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :out".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":out must be string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_gpk_in(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":in".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :in".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":in must be string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_gpk_mode(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::symbol(":mode"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Symbol(s)) => Ok(Some(s.clone())),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(EffectsError::BadPayload(format!(
            ":mode must be symbol/string or nil, got {}",
            print_term(other)
        ))),
    }
}

fn payload_gpk_depth(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":depth"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

fn payload_gpk_refs(payload: &Term) -> Result<Vec<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(EffectsError::BadPayload(format!(
            ":refs must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            Term::Symbol(s) => out.push(s.clone()),
            other => {
                return Err(EffectsError::BadPayload(format!(
                    ":refs entries must be strings/symbols, got {}",
                    print_term(other)
                )));
            }
        }
    }
    Ok(out)
}

fn payload_data(payload: &Term) -> Result<Vec<u8>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":data".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :data".to_string()))?;
    match v {
        Term::Bytes(b) => Ok(b.clone()),
        Term::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(EffectsError::BadPayload(format!(
            ":data must be bytes or string, got {}",
            print_term(v)
        ))),
    }
}

fn effective_base_dir(pol: Option<&OpPolicy>) -> Result<PathBuf, EffectsError> {
    if let Some(pol) = pol
        && let Some(base) = &pol.base_dir
    {
        return Ok(base.clone());
    }
    Ok(std::env::current_dir()?)
}

fn sandbox_path_read(base_dir: &Path, input: &str) -> Result<PathBuf, EffectsError> {
    let base = std::fs::canonicalize(base_dir)?;
    let p = PathBuf::from(input);
    // Reject any `..` to prevent traversal. We allow absolute paths only within base.
    for c in p.components() {
        if matches!(c, std::path::Component::ParentDir) {
            return Err(EffectsError::BadPayload(
                "path must not contain '..'".to_string(),
            ));
        }
    }
    let candidate = if p.is_absolute() { p } else { base.join(p) };
    // Resolve symlinks and ensure the result stays inside the base.
    let resolved = std::fs::canonicalize(&candidate)?;
    if !resolved.starts_with(&base) {
        return Err(EffectsError::BadPayload(format!(
            "path escapes base dir: {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

fn sandbox_path_write(
    base_dir: &Path,
    input: &str,
    create_dirs: bool,
) -> Result<PathBuf, EffectsError> {
    let base = std::fs::canonicalize(base_dir)?;
    let p = PathBuf::from(input);
    for c in p.components() {
        if matches!(c, std::path::Component::ParentDir) {
            return Err(EffectsError::BadPayload(
                "path must not contain '..'".to_string(),
            ));
        }
    }
    let candidate = if p.is_absolute() { p } else { base.join(p) };
    // Validate parent directory is under base once directories exist.
    if let Some(parent) = candidate.parent() {
        if create_dirs {
            std::fs::create_dir_all(parent)?;
        }
        let parent_resolved = std::fs::canonicalize(parent)?;
        if !parent_resolved.starts_with(&base) {
            return Err(EffectsError::BadPayload(format!(
                "path escapes base dir: {}",
                candidate.display()
            )));
        }
    }
    Ok(candidate)
}

fn atomic_write_text(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp_i: u64 = 0;
    let tmp_path = loop {
        let cand = dir.join(format!(
            ".tmp-{}-{}-{}",
            std::process::id(),
            path.file_name().and_then(|s| s.to_str()).unwrap_or("lock"),
            tmp_i
        ));
        tmp_i = tmp_i.saturating_add(1);
        match OpenOptions::new().write(true).create_new(true).open(&cand) {
            Ok(mut f) => {
                f.write_all(bytes)?;
                f.sync_all()?;
                break cand;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };

    match std::fs::rename(&tmp_path, path) {
        Ok(()) => {
            #[cfg(unix)]
            {
                let d = std::fs::File::open(dir)?;
                d.sync_all()?;
            }
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

#[derive(Debug, Clone)]
enum Selector {
    Commit(String),
    Snapshot(String),
    Ref(String),
}

fn parse_selector(s: &str) -> Option<Selector> {
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

fn resolve_requirement(
    store: &ArtifactStore,
    refs: &RefsDb,
    _name: &str,
    req: &gc_pkg::Requirement,
    error_tok: SealId,
    op: &str,
) -> Result<gc_pkg::LockedEntry, Value> {
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
            Ok(gc_pkg::LockedEntry {
                commit: None,
                snapshot: h,
                registry: req.registry.clone(),
                source_selector: req.selector.clone(),
                resolved_ref: None,
                exports_hash: None,
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
            Ok(gc_pkg::LockedEntry {
                commit: Some(h),
                snapshot: c.result,
                registry: req.registry.clone(),
                source_selector: req.selector.clone(),
                resolved_ref: None,
                exports_hash: None,
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
            Ok(gc_pkg::LockedEntry {
                commit: Some(commit_hex),
                snapshot: c.result,
                registry: req.registry.clone(),
                source_selector: req.selector.clone(),
                resolved_ref: Some(rn),
                exports_hash: None,
            })
        }
    }
}

fn payload_pkg_lock(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":lock"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        None => Ok("genesis.lock".to_string()),
        Some(other) => Err(format!(":lock must be string, got {}", print_term(other))),
    }
}

fn payload_pkg_workspace(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":workspace"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :workspace string".to_string()),
    }
}

fn payload_pkg_policy(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":policy"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn payload_pkg_registry_default(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":registry-default"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn payload_pkg_name(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":name"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :name string".to_string()),
    }
}

fn payload_pkg_selector(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":selector"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :selector string".to_string()),
    }
}

fn payload_pkg_registry(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":registry"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn payload_pkg_update_policy(payload: &Term) -> Result<Option<gc_pkg::UpdatePolicy>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":update-policy"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => match s.as_str() {
            "auto" => Ok(Some(gc_pkg::UpdatePolicy::Auto)),
            "manual" => Ok(Some(gc_pkg::UpdatePolicy::Manual)),
            other => Err(format!(
                ":update-policy must be 'manual' or 'auto', got {other}"
            )),
        },
        Some(other) => Err(format!(
            ":update-policy must be string or nil, got {}",
            print_term(other)
        )),
    }
}

fn payload_pkg_bool(payload: &Term, key: &str) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct SyncPolicy {
    remote_allow: Vec<String>,
    allow_http: bool,
}

fn sync_policy_from_op(pol: Option<&OpPolicy>) -> Result<SyncPolicy, String> {
    let mut remote_allow: Vec<String> = Vec::new();
    let mut allow_http = false;
    if let Some(pol) = pol {
        if let Some(v) = pol.extra.get("remote_allow")
            && let Some(arr) = v.as_array()
        {
            for x in arr {
                let s = x
                    .as_str()
                    .ok_or_else(|| "remote_allow entries must be strings".to_string())?;
                let t = s.trim();
                if !t.is_empty() {
                    remote_allow.push(t.to_string());
                }
            }
        }
        if let Some(v) = pol.extra.get("allow_http")
            && let Some(b) = v.as_bool()
        {
            allow_http = b;
        }
    }
    if remote_allow.is_empty() {
        return Err("sync requires per-op remote_allow allowlist in caps.toml".to_string());
    }
    Ok(SyncPolicy {
        remote_allow,
        allow_http,
    })
}

fn sync_normalize_and_check_remote(sp: &SyncPolicy, remote: &str) -> Result<String, String> {
    let base = gc_registry::normalize_remote_base(remote).map_err(|e| format!("{e}"))?;
    if base.scheme() == "http" && !sp.allow_http {
        return Err("http remotes are disabled by policy (set allow_http=true)".to_string());
    }
    let base_s = base.as_str().to_string();
    for p in &sp.remote_allow {
        if base_s.starts_with(p) {
            return Ok(base_s);
        }
    }
    Err("remote is not in policy remote_allow allowlist".to_string())
}

fn payload_sync_remote(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":remote"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :remote string".to_string()),
    }
}

fn payload_sync_refs(payload: &Term) -> Result<Vec<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(":refs must be vector, got {}", print_term(t)));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            other => {
                return Err(format!(
                    ":refs entries must be strings, got {}",
                    print_term(other)
                ));
            }
        }
    }
    Ok(out)
}

fn payload_sync_roots(payload: &Term) -> Result<Vec<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":roots"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(":roots must be vector, got {}", print_term(t)));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            other => {
                return Err(format!(
                    ":roots entries must be strings, got {}",
                    print_term(other)
                ));
            }
        }
    }
    Ok(out)
}

fn payload_sync_depth(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":depth"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

fn payload_sync_force(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":force"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct SyncSetRef {
    name: String,
    hash: String,
    policy: String,
    expected_old: Option<String>,
}

fn payload_sync_set_refs(payload: &Term) -> Result<Vec<SyncSetRef>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":set-refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(":set-refs must be vector, got {}", print_term(t)));
    };
    let mut out = Vec::new();
    for x in xs {
        let Term::Map(mm) = x else {
            return Err(format!(
                ":set-refs entries must be maps, got {}",
                print_term(x)
            ));
        };
        let name = match mm.get(&TermOrdKey(Term::symbol(":name"))) {
            Some(Term::Str(s)) => s.clone(),
            _ => return Err("set-ref missing :name string".to_string()),
        };
        let hash = match mm.get(&TermOrdKey(Term::symbol(":hash"))) {
            Some(Term::Str(s)) => s.clone(),
            _ => return Err("set-ref missing :hash string".to_string()),
        };
        let policy = match mm.get(&TermOrdKey(Term::symbol(":policy"))) {
            Some(Term::Str(s)) => s.clone(),
            _ => return Err("set-ref missing :policy string".to_string()),
        };
        let expected_old = match mm.get(&TermOrdKey(Term::symbol(":expected-old"))) {
            None | Some(Term::Nil) => None,
            Some(Term::Str(s)) => Some(s.clone()),
            Some(other) => {
                return Err(format!(
                    "set-ref :expected-old must be string or nil, got {}",
                    print_term(other)
                ));
            }
        };
        out.push(SyncSetRef {
            name,
            hash,
            policy,
            expected_old,
        });
    }
    Ok(out)
}

struct SyncPullStats<'a> {
    pulled: &'a mut u64,
    already: &'a mut u64,
    error_tok: SealId,
    op: &'a str,
}

fn sync_pull_closure(
    client: &gc_registry::RegistryClient,
    store: &ArtifactStore,
    root: &str,
    depth: u64,
    stats: &mut SyncPullStats<'_>,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};

    let mut q: VecDeque<(String, u64)> = VecDeque::new();
    q.push_back((root.to_string(), depth));
    let mut seen: HashSet<String> = HashSet::new();
    let mut obj_count: u64 = 0;

    while let Some((h, dleft)) = q.pop_front() {
        if !seen.insert(h.clone()) {
            continue;
        }
        obj_count = obj_count.saturating_add(1);
        if obj_count > 50_000 {
            return Err(mk_error(
                stats.error_tok,
                "core/sync/too-many-objects",
                "closure exceeded 50k objects".to_string(),
                Some(stats.op),
            ));
        }

        if store.path_for(&h).exists() {
            if store.verify_hex(&h).is_err() {
                return Err(mk_error(
                    stats.error_tok,
                    "core/store/corruption",
                    format!("artifact store corruption: {h}"),
                    Some(stats.op),
                ));
            }
            *stats.already = stats.already.saturating_add(1);
        } else {
            let bytes = client.store_get(&h).map_err(|e| {
                mk_error(
                    stats.error_tok,
                    "core/sync/remote-error",
                    format!("{e}"),
                    Some(stats.op),
                )
            })?;
            let got = store
                .put_bytes(&bytes)
                .map_err(|e| {
                    mk_error(stats.error_tok, "core/store/io-error", e.to_string(), Some(stats.op))
                })?;
            if got != h {
                return Err(mk_error(
                    stats.error_tok,
                    "core/sync/hash-mismatch",
                    "remote bytes hash mismatch".to_string(),
                    Some(stats.op),
                ));
            }
            *stats.pulled = stats.pulled.saturating_add(1);
        }

        let t = match store_get_term(store, &h) {
            Ok(t) => t,
            Err(_) => continue,
        };

        // Commit closure: commit, base, patch, result snapshot, evidence, attestations, parents.
        if let Ok(c) = gc_vcs::Commit::from_term(&t) {
            if let Some(b) = c.base {
                q.push_back((b, dleft));
            }
            q.push_back((c.patch, dleft));
            q.push_back((c.result, dleft));
            for x in c.evidence {
                q.push_back((x, dleft));
            }
            for x in c.attestations {
                q.push_back((x, dleft));
            }
            if dleft > 0 {
                for p in c.parents {
                    q.push_back((p, dleft - 1));
                }
            }
            continue;
        }

        // Patch closure: follow referenced values.
        if let Ok(p) = gc_vcs::Patch::from_term(&t) {
            for x in p.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }

        // Evidence closure: follow any referenced inputs/outputs/data.
        if let Ok(e) = gc_vcs::Evidence::from_term(&t) {
            for x in e.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }

        // Snapshot closure: shallow refs.
        if let Ok(s) = gc_vcs::Snapshot::from_term(&t) {
            for x in s.shallow_refs() {
                q.push_back((x, dleft));
            }
        }
    }

    Ok(())
}

fn sync_closure_local(
    store: &ArtifactStore,
    root: &str,
    depth: u64,
    out: &mut std::collections::BTreeSet<String>,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};
    let mut q: VecDeque<(String, u64)> = VecDeque::new();
    q.push_back((root.to_string(), depth));
    let mut seen: HashSet<String> = HashSet::new();
    let mut obj_count: u64 = 0;

    while let Some((h, dleft)) = q.pop_front() {
        if !seen.insert(h.clone()) {
            continue;
        }
        obj_count = obj_count.saturating_add(1);
        if obj_count > 50_000 {
            return Err(mk_error(
                error_tok,
                "core/sync/too-many-objects",
                "closure exceeded 50k objects".to_string(),
                Some(op),
            ));
        }
        if !store.path_for(&h).exists() {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {h}"),
                Some(op),
            ));
        }
        if store.verify_hex(&h).is_err() {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact store corruption: {h}"),
                Some(op),
            ));
        }
        out.insert(h.clone());

        let t = match store_get_term(store, &h) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Ok(c) = gc_vcs::Commit::from_term(&t) {
            if let Some(b) = c.base {
                q.push_back((b, dleft));
            }
            q.push_back((c.patch, dleft));
            q.push_back((c.result, dleft));
            for x in c.evidence {
                q.push_back((x, dleft));
            }
            for x in c.attestations {
                q.push_back((x, dleft));
            }
            if dleft > 0 {
                for p in c.parents {
                    q.push_back((p, dleft - 1));
                }
            }
            continue;
        }
        if let Ok(p) = gc_vcs::Patch::from_term(&t) {
            for x in p.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(e) = gc_vcs::Evidence::from_term(&t) {
            for x in e.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(s) = gc_vcs::Snapshot::from_term(&t) {
            for x in s.shallow_refs() {
                q.push_back((x, dleft));
            }
        }
    }
    Ok(())
}
