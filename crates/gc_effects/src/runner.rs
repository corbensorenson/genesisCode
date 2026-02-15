use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use blake3::Hasher;
use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EffectProgram, EffectRequest, EvalCtx, SealId, Value, value_hash};
use num_bigint::BigInt;

use crate::error::EffectsError;
use crate::log::{Decision, EffectLog, EffectLogEntry, LoggedResp};
use crate::policy::{CapsPolicy, OpPolicy};
use crate::store::ArtifactStore;

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

    let store = match policy.store_dir() {
        Some(sd) => Some(ArtifactStore::open(sd)?),
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
                    let resp = call_capability(&req.op, &req.payload, pol, proto.error)?;
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
