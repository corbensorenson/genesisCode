use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use blake3::Hasher;
use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EffectProgram, EffectRequest, EvalCtx, SealId, Value, value_hash};
use num_bigint::BigInt;

use crate::error::EffectsError;
use crate::log::{Decision, EffectLog, EffectLogEntry, LoggedResp};
use crate::policy::{CapsPolicy, OpPolicy};

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
                    version: 1,
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
                        logged_resp(&resp, proto.error)?,
                    )
                } else {
                    let pol = policy.op_policy(&req.op);
                    let cap_term = cap_term(&req.op, pol)?;
                    let resp = call_capability(&req.op, &req.payload, pol, proto.error)?;
                    (
                        Decision::Allow,
                        cap_term,
                        resp.clone(),
                        logged_resp(&resp, proto.error)?,
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

                let resp_val = match &entry.resp {
                    LoggedResp::Ok(t) => Value::Data(t.clone()),
                    LoggedResp::Error(payload) => Value::Sealed {
                        token: proto.error,
                        payload: Box::new(Value::Data(payload.clone())),
                    },
                };

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

fn logged_resp(v: &Value, error_tok: SealId) -> Result<LoggedResp, EffectsError> {
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

fn cap_term(op: &str, pol: Option<&OpPolicy>) -> Result<Term, EffectsError> {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":op".to_string())),
        Term::Symbol(op.to_string()),
    );
    if let Some(pol) = pol {
        if let Some(base) = &pol.base_dir {
            m.insert(
                TermOrdKey(Term::Symbol(":base-dir".to_string())),
                Term::Str(base.display().to_string()),
            );
        }
        if pol.create_dirs {
            m.insert(
                TermOrdKey(Term::Symbol(":create-dirs".to_string())),
                Term::Bool(true),
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
    match op {
        "sys/time::now" => {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            Ok(Value::Data(Term::Int(BigInt::from(ms))))
        }
        "io/fs::read" => {
            let path_s = payload_path(payload)?;
            let base_dir = effective_base_dir(pol)?;
            let path = sandbox_path_read(&base_dir, &path_s)?;
            match std::fs::read(&path) {
                Ok(bytes) => Ok(Value::Data(Term::Bytes(bytes))),
                Err(e) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &path, &e))),
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
                        payload: Box::new(Value::Data(io_error_payload(op, &path, &e))),
                    });
                }
            }
            match std::fs::write(&path, data) {
                Ok(()) => Ok(Value::Data(Term::Nil)),
                Err(e) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &path, &e))),
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

fn io_error_payload(op: &str, path: &Path, e: &std::io::Error) -> Term {
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
                    Term::Str(path.display().to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    );
    Term::Map(m)
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
