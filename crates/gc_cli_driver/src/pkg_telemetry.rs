use gc_coreform::Term;
use gc_effects::EffectLog;
use gc_kernel::{Value, value_hash};

use super::{PkgCmd, pkg_contract};

pub(crate) fn build_pkg_telemetry(
    cmd: &PkgCmd,
    ok: bool,
    exit_code: u8,
    log: &EffectLog,
    value: &Value,
    report: Option<&serde_json::Value>,
    doctor: Option<&serde_json::Value>,
) -> serde_json::Value {
    let log_bytes = log.to_string_canonical();
    let log_hash = blake3::hash(log_bytes.as_bytes());
    let value_h = value_hash(value);

    let mut telemetry = serde_json::json!({
        "schema": "genesis/pkg-telemetry-v0.1",
        "command": pkg_contract::log_op(cmd),
        "ok": ok,
        "exit_code": exit_code,
        "effect_log_hash": hex32(log_hash.as_bytes()),
        "value_hash": hex32(&value_h),
        "effect_entries": log.entries.len(),
        "value_kind": value_kind(value),
    });

    if let Some(obj) = telemetry.as_object_mut() {
        if let Some(changed) = report
            .and_then(|r| r.get("changed"))
            .and_then(|x| x.as_bool())
        {
            obj.insert("changed".to_string(), serde_json::Value::Bool(changed));
        }
        if let Some(issues) = doctor
            .and_then(|r| r.get("issue_count"))
            .and_then(|x| x.as_u64())
        {
            obj.insert(
                "doctor_issue_count".to_string(),
                serde_json::Value::from(issues),
            );
        }
    }

    telemetry
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Data(t) => term_kind(t.as_ref()),
        Value::Int(_) => "int",
        Value::Closure(_) => "closure",
        Value::CompiledClosure(_) => "compiled-closure",
        Value::SealToken(_) => "seal-token",
        Value::Sealed { .. } => "sealed",
        Value::NativeFn(_) => "native-fn",
        Value::Contract(_) => "contract",
        Value::EffectProgram(_) => "effect-program",
        Value::EffectRequest { .. } => "effect-request",
        Value::Map(_) => "map",
        Value::Vector(_) => "vector",
    }
}

fn term_kind(t: &Term) -> &'static str {
    match t {
        Term::Nil => "nil",
        Term::Bool(_) => "bool",
        Term::Int(_) => "int",
        Term::Str(_) => "str",
        Term::Bytes(_) => "bytes",
        Term::Symbol(_) => "symbol",
        Term::Pair(_, _) => "pair",
        Term::Vector(_) => "vector",
        Term::Map(_) => "map",
    }
}

fn hex32(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use gc_coreform::{Term, TermOrdKey};
    use gc_effects::{Decision, EffectLog, EffectLogEntry, LoggedResp};
    use gc_kernel::Value;

    use super::build_pkg_telemetry;
    use crate::PkgCmd;

    #[test]
    fn telemetry_is_deterministic_and_prompt_safe() {
        let log = EffectLog {
            version: 2,
            program_hash: [7; 32],
            toolchain: "genesis test".to_string(),
            entries: vec![EffectLogEntry {
                i: 0,
                op: "core/pkg-low::lock".to_string(),
                payload_h: [1; 32],
                cont_h: [2; 32],
                req_h: [3; 32],
                task_id: None,
                parent_task: None,
                schedule_step: None,
                await_edge: None,
                decision: Decision::Allow,
                cap: Term::symbol("core/pkg-low::lock"),
                resp: LoggedResp::Ok(Term::Map(BTreeMap::new())),
                resp_h: [4; 32],
            }],
        };
        let value = Value::data(Term::Map(
            [(
                TermOrdKey(Term::symbol(":lock-h")),
                Term::Str("a".repeat(64)),
            )]
            .into_iter()
            .collect(),
        ));
        let cmd = PkgCmd::Lock {
            lock: PathBuf::from("genesis.lock"),
            strict: true,
        };
        let t1 = build_pkg_telemetry(&cmd, true, 0, &log, &value, None, None);
        let t2 = build_pkg_telemetry(&cmd, true, 0, &log, &value, None, None);
        assert_eq!(t1, t2);
        assert_eq!(
            t1.get("schema").and_then(|v| v.as_str()),
            Some("genesis/pkg-telemetry-v0.1")
        );
        assert_eq!(t1.get("command").and_then(|v| v.as_str()), Some("pkg-lock"));
    }
}
