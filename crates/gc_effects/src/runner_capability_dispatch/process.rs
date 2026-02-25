use super::*;

fn process_allow_programs_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Vec<String>, String> {
    let Some(pol) = pol else {
        return Err(format!(
            "{op} requires per-op allow_programs allowlist in caps.toml"
        ));
    };
    let Some(v) = pol.extra.get("allow_programs") else {
        return Err(format!(
            "{op} requires per-op allow_programs allowlist in caps.toml"
        ));
    };
    let Some(arr) = v.as_array() else {
        return Err("allow_programs must be an array of strings".to_string());
    };
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        let Some(raw) = x.as_str() else {
            return Err("allow_programs entries must be strings".to_string());
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        return Err("allow_programs must contain at least one program name".to_string());
    }
    Ok(out)
}

pub(super) fn capability_sys_process_spawn_or_exec(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let program = payload_required_string_field(payload, op, ":program")?;
    let allow_programs = match process_allow_programs_from_policy(pol, op) {
        Ok(v) => v,
        Err(e) => {
            return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
        }
    };
    if !allowlist_contains_exact_or_glob(&allow_programs, &program) {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "sys/process::exec denied for program `{program}`; configure allow_programs in caps.toml op policy"
            ),
            Some(op),
        ));
    }
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("process", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_sys_process_wait_or_kill(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _process_id = payload_required_string_field(payload, op, ":process-id")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("process", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_sys_process_stdin_write(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let _process_id = payload_required_string_field(payload, op, ":process-id")?;
    let _data = payload_required_field(payload, op, ":data")?;
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge("process", op, payload, pol) {
        Ok(resp) => Ok(Value::Data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}
