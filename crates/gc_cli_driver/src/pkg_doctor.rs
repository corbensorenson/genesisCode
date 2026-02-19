use std::path::Path;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{EvalCtx, Value};

pub(crate) struct DoctorReport {
    pub(crate) ok: bool,
    pub(crate) json: serde_json::Value,
}

struct LockDrift {
    missing_locked: Vec<String>,
    stale_locked: Vec<String>,
    selector_mismatch: Vec<serde_json::Value>,
}

pub(crate) fn build_pkg_doctor_report(
    ctx: &EvalCtx,
    v: &Value,
    caps: &Path,
    lock: &Path,
    base_ok: bool,
    exit_code: u8,
) -> DoctorReport {
    let mut checks = Vec::<serde_json::Value>::new();
    let mut fixes = Vec::<serde_json::Value>::new();

    checks.push(serde_json::json!({
        "id": "caps.parse",
        "ok": true,
        "severity": "info",
        "message": "capability policy parsed"
    }));

    match inspect_lock_drift(lock) {
        Ok(drift) => {
            let drift_count = drift.missing_locked.len()
                + drift.stale_locked.len()
                + drift.selector_mismatch.len();
            let drift_ok = drift_count == 0;
            checks.push(serde_json::json!({
                "id": "lock.drift",
                "ok": drift_ok,
                "severity": if drift_ok { "info" } else { "error" },
                "message": if drift_ok {
                    "lock requirements and locked entries are aligned"
                } else {
                    "lock drift detected between requirements and locked entries"
                },
                "drift_count": drift_count,
                "missing_locked": drift.missing_locked,
                "stale_locked": drift.stale_locked,
                "selector_mismatch": drift.selector_mismatch
            }));
            if !drift_ok {
                fixes.push(serde_json::json!({
                    "id": "rebuild-lock",
                    "action": {
                        "op": "gcpm.lock",
                        "args": {
                            "lock": lock.display().to_string(),
                            "strict": true
                        }
                    },
                    "command": format!(
                        "genesis gcpm --caps {} lock --lock {} --strict",
                        caps.display(),
                        lock.display()
                    ),
                    "why": "re-resolve requirements into a deterministic locked set and clear drift"
                }));
            }
        }
        Err(e) => {
            checks.push(serde_json::json!({
                "id": "lock.parse",
                "ok": false,
                "severity": "error",
                "message": "unable to parse lock file",
                "error": e
            }));
            fixes.push(serde_json::json!({
                "id": "init-lock",
                "action": {
                    "op": "gcpm.init",
                    "args": {
                        "workspace": "ws",
                        "lock": lock.display().to_string()
                    }
                },
                "command": format!(
                    "genesis gcpm --caps {} init --workspace ws --lock {}",
                    caps.display(),
                    lock.display()
                ),
                "why": "create a canonical lock file before running install/verify workflows"
            }));
        }
    }

    if let Some(lock_ok) = extract_pkg_ok_bool(v) {
        checks.push(serde_json::json!({
            "id": "lock.verify",
            "ok": lock_ok,
            "severity": if lock_ok { "info" } else { "error" },
            "message": if lock_ok { "lock verifies cleanly" } else { "lock verification failed" }
        }));
        if let Some(missing_count) = extract_pkg_missing_count(v) {
            let missing_ok = missing_count == 0;
            checks.push(serde_json::json!({
                "id": "store.artifacts",
                "ok": missing_ok,
                "severity": if missing_ok { "info" } else { "error" },
                "message": if missing_ok {
                    "all referenced artifacts are present".to_string()
                } else {
                    format!("{} referenced artifacts are missing", missing_count)
                },
                "missing_count": missing_count
            }));
            if !missing_ok {
                fixes.push(serde_json::json!({
                    "id": "materialize-artifacts",
                    "action": {
                        "op": "gcpm.install",
                        "args": {
                            "lock": lock.display().to_string(),
                            "strict": true
                        }
                    },
                    "command": format!(
                        "genesis gcpm --caps {} install --lock {} --strict",
                        caps.display(),
                        lock.display()
                    ),
                    "why": "materialize and validate missing artifacts"
                }));
            }
        }
    }

    if let Some(code) = extract_sealed_error_code(ctx, v) {
        let code_for_report = code.clone();
        checks.push(serde_json::json!({
            "id": "effects.execution",
            "ok": false,
            "severity": "error",
            "message": format!("effect program returned sealed error: {}", code_for_report),
            "code": code_for_report
        }));
        if code == "core/caps/denied" {
            fixes.push(serde_json::json!({
                "id": "allow-required-ops",
                "action": {
                    "op": "caps.allow",
                    "args": {
                        "ops": ["core/pkg-low::verify", "core/pkg-low::load-lock", "core/store::has", "core/store::get"]
                    }
                },
                "command": "update caps.toml allowlist for core/pkg-low::verify, core/pkg-low::load-lock, core/store::{has,get}",
                "why": "doctor requires lock loading and artifact-presence checks"
            }));
        } else if code == "core/pkg/not-locked" {
            fixes.push(serde_json::json!({
                "id": "generate-lock",
                "action": {
                    "op": "gcpm.lock",
                    "args": {
                        "lock": lock.display().to_string(),
                        "strict": false
                    }
                },
                "command": format!(
                    "genesis gcpm --caps {} lock --lock {}",
                    caps.display(),
                    lock.display()
                ),
                "why": "create locked entries for all requirements before install/verify"
            }));
        }
    }

    let issue_count = checks
        .iter()
        .filter(|c| c.get("ok").and_then(|v| v.as_bool()) == Some(false))
        .count();
    let final_ok = base_ok && issue_count == 0;

    let report = serde_json::json!({
        "schema": "genesis/pkg-doctor-report-v0.2",
        "ok": final_ok,
        "base_ok": base_ok,
        "issue_count": issue_count,
        "exit_code": exit_code,
        "lock": lock.display().to_string(),
        "caps": caps.display().to_string(),
        "checks": checks,
        "fixes": fixes
    });

    DoctorReport {
        ok: final_ok,
        json: report,
    }
}

fn inspect_lock_drift(lock_path: &Path) -> Result<LockDrift, String> {
    let lock = gc_pkg::GenesisLock::load(lock_path).map_err(|e| e.to_string())?;

    let mut missing_locked = Vec::new();
    let mut stale_locked = Vec::new();
    let mut selector_mismatch = Vec::new();

    for (name, req) in &lock.requirements {
        match lock.locked.get(name) {
            None => missing_locked.push(name.clone()),
            Some(entry) => {
                if !entry.source_selector.is_empty() && entry.source_selector != req.selector {
                    selector_mismatch.push(serde_json::json!({
                        "name": name,
                        "requirement_selector": req.selector,
                        "locked_source_selector": entry.source_selector
                    }));
                }
            }
        }
    }

    for name in lock.locked.keys() {
        if !lock.requirements.contains_key(name) {
            stale_locked.push(name.clone());
        }
    }

    Ok(LockDrift {
        missing_locked,
        stale_locked,
        selector_mismatch,
    })
}

fn extract_pkg_ok_bool(v: &Value) -> Option<bool> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&TermOrdKey(Term::symbol(":ok"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn extract_pkg_missing_count(v: &Value) -> Option<usize> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&TermOrdKey(Term::symbol(":missing"))) {
        Some(Term::Vector(xs)) => Some(xs.len()),
        _ => None,
    }
}

fn extract_sealed_error_code(ctx: &EvalCtx, v: &Value) -> Option<String> {
    let proto = ctx.protocol?;
    let Value::Sealed { token, payload } = v else {
        return None;
    };
    if *token != proto.error {
        return None;
    }
    match payload.as_ref() {
        Value::Data(Term::Map(m)) => {
            m.get(&TermOrdKey(Term::symbol(":error/code")))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.clone()),
                    _ => None,
                })
        }
        Value::Map(m) => m
            .get(&TermOrdKey(Term::symbol(":error/code")))
            .and_then(|vv| match vv {
                Value::Data(Term::Str(s)) => Some(s.clone()),
                _ => None,
            }),
        _ => None,
    }
}
