use super::*;

#[derive(Debug, Clone, Default)]
pub(super) struct RuntimeBudgetState {
    effect_ops: u64,
    payload_bytes: usize,
    response_bytes: usize,
}

pub(super) fn enforce_request_runtime_limits(
    policy: &CapsPolicy,
    budget: &mut RuntimeBudgetState,
    op: &str,
    payload: &Term,
    error_tok: SealId,
) -> Option<Value> {
    let observed_effect_ops = budget.effect_ops.saturating_add(1);
    budget.effect_ops = observed_effect_ops;
    if let Some(limit) = policy.runtime.max_effect_ops
        && observed_effect_ops > limit
    {
        return Some(runtime_limit_error(
            error_tok,
            op,
            "max_effect_ops",
            observed_effect_ops as usize,
            limit as usize,
            "ops",
        ));
    }

    let payload_bytes = print_term(payload).len();
    if let Some(limit) = policy.runtime.max_payload_bytes_per_op
        && payload_bytes > limit
    {
        return Some(runtime_limit_error(
            error_tok,
            op,
            "max_payload_bytes_per_op",
            payload_bytes,
            limit,
            "bytes",
        ));
    }

    if let Some(limit) = policy.runtime.max_payload_bytes_per_run {
        let observed = budget.payload_bytes.saturating_add(payload_bytes);
        if observed > limit {
            return Some(runtime_limit_error(
                error_tok,
                op,
                "max_payload_bytes_per_run",
                observed,
                limit,
                "bytes",
            ));
        }
        budget.payload_bytes = observed;
    } else {
        budget.payload_bytes = budget.payload_bytes.saturating_add(payload_bytes);
    }

    None
}

pub(super) fn enforce_response_runtime_limits(
    policy: &CapsPolicy,
    budget: &mut RuntimeBudgetState,
    op: &str,
    response: &Value,
    error_tok: SealId,
) -> Result<Option<Value>, EffectsError> {
    let response_bytes = response_serialized_bytes(response)?;
    if let Some(limit) = policy.runtime.max_response_bytes_per_op
        && response_bytes > limit
    {
        return Ok(Some(runtime_limit_error(
            error_tok,
            op,
            "max_response_bytes_per_op",
            response_bytes,
            limit,
            "bytes",
        )));
    }

    if let Some(limit) = policy.runtime.max_response_bytes_per_run {
        let observed = budget.response_bytes.saturating_add(response_bytes);
        if observed > limit {
            return Ok(Some(runtime_limit_error(
                error_tok,
                op,
                "max_response_bytes_per_run",
                observed,
                limit,
                "bytes",
            )));
        }
        budget.response_bytes = observed;
    } else {
        budget.response_bytes = budget.response_bytes.saturating_add(response_bytes);
    }

    Ok(None)
}

fn response_serialized_bytes(response: &Value) -> Result<usize, EffectsError> {
    match response {
        Value::Data(term) => Ok(print_term(term).len()),
        Value::Sealed { payload, .. } => {
            let Value::Data(term) = payload.as_ref() else {
                return Err(EffectsError::Log(
                    "sealed response payload must be datum for runtime budgeting".to_string(),
                ));
            };
            Ok(print_term(term).len())
        }
        other => Err(EffectsError::Log(format!(
            "response not serializable for runtime budgeting: {}",
            other.debug_repr()
        ))),
    }
}

fn runtime_limit_error(
    error_tok: SealId,
    op: &str,
    budget_name: &str,
    observed: usize,
    limit: usize,
    unit: &str,
) -> Value {
    let mut ctx = BTreeMap::new();
    ctx.insert(
        TermOrdKey(Term::symbol(":runtime/budget")),
        Term::Str(budget_name.to_string()),
    );
    ctx.insert(
        TermOrdKey(Term::symbol(":runtime/unit")),
        Term::Str(unit.to_string()),
    );
    ctx.insert(
        TermOrdKey(Term::symbol(":runtime/observed")),
        Term::Int(BigInt::from(observed)),
    );
    ctx.insert(
        TermOrdKey(Term::symbol(":runtime/limit")),
        Term::Int(BigInt::from(limit)),
    );

    mk_error_with_ctx(
        error_tok,
        "core/caps/resource-limit",
        format!(
            "runtime policy limit exceeded: {budget_name} observed {observed} > {limit} {unit} for {op}"
        ),
        Some(op),
        Term::Map(ctx),
    )
}
