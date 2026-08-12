use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};

use crate::error::EffectsError;

use super::{CapsPolicy, OpPolicy};

#[path = "policy_authority_resource.rs"]
mod resource;

const MAX_POLICY_OPS: usize = 4_096;
const POLICY_AUTHORITY_STEP_LIMIT: u64 = 20_000_000;
const POLICY_AUTHORITY_ALLOC_LIMIT: u64 = 20_000_000;

fn authority_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!(
        "caps.toml: selfhost policy authority: {}",
        message.into()
    ))
}

fn hex32(hash: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in hash {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn optional_bool(value: Option<&toml::Value>) -> Term {
    value
        .and_then(toml::Value::as_bool)
        .map(Term::Bool)
        .unwrap_or(Term::Nil)
}

fn optional_int(value: Option<&toml::Value>) -> Term {
    value
        .and_then(toml::Value::as_integer)
        .map(|number| Term::Int(number.into()))
        .unwrap_or(Term::Nil)
}

fn override_term(value: Option<&toml::Value>) -> Result<Term, EffectsError> {
    let Some(value) = value else {
        return Ok(Term::Nil);
    };
    let table = value
        .as_table()
        .ok_or_else(|| authority_error("operation override must be a table"))?;
    Ok(Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":allow")),
                optional_bool(table.get("allow")),
            ),
            (
                TermOrdKey(Term::symbol(":create-dirs")),
                optional_bool(table.get("create_dirs")),
            ),
            (
                TermOrdKey(Term::symbol(":log-inline-max-bytes")),
                optional_int(table.get("log_inline_max_bytes")),
            ),
            (
                TermOrdKey(Term::symbol(":timeout-ms")),
                optional_int(table.get("timeout_ms")),
            ),
        ]
        .into_iter()
        .collect(),
    ))
}

fn request_term(op: &str, baseline: &[String], override_value: Term) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":baseline")),
                Term::Vector(baseline.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/effect-policy-authority-request-v0.1".to_string()),
            ),
            (TermOrdKey(Term::symbol(":op")), Term::Str(op.to_string())),
            (TermOrdKey(Term::symbol(":override")), override_value),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn inventory_request_term(baseline: &[String], override_ops: &[String]) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":baseline")),
                Term::Vector(baseline.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/effect-policy-inventory-request-v0.1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":override-ops")),
                Term::Vector(override_ops.iter().cloned().map(Term::Str).collect()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn decode_inventory_result(
    term: Term,
    request_hash: [u8; 32],
) -> Result<Vec<String>, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error("inventory result must be a data map"));
    };
    let expected_keys: BTreeSet<_> = [":candidate-ops", ":kind", ":request-h", ":v"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected_keys {
        return Err(authority_error("inventory result field set mismatch"));
    }
    if !matches!(map.get(&TermOrdKey(Term::symbol(":kind"))), Some(Term::Str(kind)) if kind == "genesis/effect-policy-inventory-result-v0.1")
        || !matches!(map.get(&TermOrdKey(Term::symbol(":v"))), Some(Term::Int(version)) if version == &1.into())
        || !matches!(map.get(&TermOrdKey(Term::symbol(":request-h"))), Some(Term::Str(actual)) if actual == &hex32(request_hash))
    {
        return Err(authority_error("inventory result identity mismatch"));
    }
    let Some(Term::Vector(candidate_terms)) = map.get(&TermOrdKey(Term::symbol(":candidate-ops")))
    else {
        return Err(authority_error("inventory :candidate-ops must be a vector"));
    };
    if candidate_terms.len() > MAX_POLICY_OPS {
        return Err(authority_error(format!(
            "operation inventory exceeds fixed limit {MAX_POLICY_OPS}"
        )));
    }
    let candidates = candidate_terms
        .iter()
        .map(|candidate| match candidate {
            Term::Str(op) => Ok(op.clone()),
            _ => Err(authority_error(
                "inventory :candidate-ops entries must be strings",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if candidates.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(authority_error(
            "inventory :candidate-ops must be strictly ordered and unique",
        ));
    }
    Ok(candidates)
}

fn plain_authority_result(
    value: Value,
    context: &EvalCtx,
    scope: &str,
) -> Result<Term, EffectsError> {
    match &value {
        Value::Sealed { token, payload }
            if context
                .protocol
                .is_some_and(|protocol| *token == protocol.error) =>
        {
            let detail = payload
                .to_plain_term()
                .map(|term| print_term(&term))
                .unwrap_or_else(|| "<opaque-error-payload>".to_string());
            Err(authority_error(format!(
                "{scope} authority returned sealed ERROR {detail}"
            )))
        }
        _ => value
            .to_plain_term()
            .ok_or_else(|| authority_error(format!("{scope} authority returned an opaque value"))),
    }
}

fn legacy_cap(op: &str, policy: &OpPolicy) -> Term {
    let mut cap = BTreeMap::new();
    cap.insert(TermOrdKey(Term::symbol(":op")), Term::symbol(op));
    if policy.create_dirs {
        cap.insert(TermOrdKey(Term::symbol(":create-dirs")), Term::Bool(true));
    }
    if let Some(timeout_ms) = policy.timeout_ms {
        cap.insert(
            TermOrdKey(Term::symbol(":timeout-ms")),
            Term::Int(timeout_ms.into()),
        );
    }
    if let Some(limit) = policy.log_inline_max_bytes {
        cap.insert(
            TermOrdKey(Term::symbol(":log-inline-max-bytes")),
            Term::Int(limit.into()),
        );
    }
    Term::Map(cap)
}

fn decode_result(
    term: Term,
    op: &str,
    request_hash: [u8; 32],
) -> Result<(bool, Term), EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error("result must be a data map"));
    };
    let expected_keys: BTreeSet<_> = [":allowed", ":cap", ":kind", ":op", ":request-h", ":v"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected_keys {
        return Err(authority_error("result field set mismatch"));
    }
    if !matches!(map.get(&TermOrdKey(Term::symbol(":kind"))), Some(Term::Str(kind)) if kind == "genesis/effect-policy-authority-result-v0.1")
        || !matches!(map.get(&TermOrdKey(Term::symbol(":v"))), Some(Term::Int(version)) if version == &1.into())
        || !matches!(map.get(&TermOrdKey(Term::symbol(":op"))), Some(Term::Str(actual)) if actual == op)
        || !matches!(map.get(&TermOrdKey(Term::symbol(":request-h"))), Some(Term::Str(actual)) if actual == &hex32(request_hash))
    {
        return Err(authority_error("result identity mismatch"));
    }
    let allowed = match map.get(&TermOrdKey(Term::symbol(":allowed"))) {
        Some(Term::Bool(value)) => *value,
        _ => return Err(authority_error("result :allowed must be bool")),
    };
    let cap = map
        .get(&TermOrdKey(Term::symbol(":cap")))
        .cloned()
        .ok_or_else(|| authority_error("result is missing :cap"))?;
    if (allowed && !matches!(cap, Term::Map(_))) || (!allowed && cap != Term::Nil) {
        return Err(authority_error("result :cap contradicts :allowed"));
    }
    Ok((allowed, cap))
}

pub(super) fn authorize_policy(
    source: &str,
    policy: &mut CapsPolicy,
    bootstrap_mode: SelfhostBootstrapMode,
    artifact: Option<&Path>,
) -> Result<(), EffectsError> {
    let document: toml::Value =
        toml::from_str(source).map_err(|error| EffectsError::Log(format!("caps.toml: {error}")))?;
    let table = document.as_table().ok_or_else(|| {
        EffectsError::Log("caps.toml: top-level value must be a table".to_string())
    })?;
    let baseline: Vec<String> = table
        .get("allow")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| authority_error("baseline allow entries must be strings"))
        })
        .collect::<Result<_, _>>()?;
    let overrides = table
        .get("op")
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default();
    let override_ops: Vec<String> = overrides.keys().cloned().collect();
    if baseline.len() > MAX_POLICY_OPS || override_ops.len() > MAX_POLICY_OPS {
        return Err(authority_error(format!(
            "operation inventory exceeds fixed limit {MAX_POLICY_OPS}"
        )));
    }

    let mut context = EvalCtx::with_step_limit(None);
    context.set_mem_limits(MemLimits {
        max_alloc_units: Some(POLICY_AUTHORITY_ALLOC_LIMIT),
        ..MemLimits::default()
    });
    let prelude = build_prelude(&mut context);
    let mut environment = prelude.env;
    load_selfhost_coreform_toolchain_v1_with_mode(
        &mut context,
        &mut environment,
        bootstrap_mode,
        artifact,
    )
    .map_err(|error| authority_error(format!("selfhost/init: {error}")))?;
    context.steps = 0;
    context.step_limit = Some(POLICY_AUTHORITY_STEP_LIMIT);
    let authority = environment
        .get("core/effects::policy-authority")
        .ok_or_else(|| authority_error("missing binding core/effects::policy-authority"))?;
    let inventory_authority = environment
        .get("core/effects::policy-inventory-authority")
        .ok_or_else(|| {
            authority_error("missing binding core/effects::policy-inventory-authority")
        })?;
    let resource_authority = environment
        .get("core/effects::resource-policy-authority")
        .ok_or_else(|| {
            authority_error("missing binding core/effects::resource-policy-authority")
        })?;

    let resource_request = resource::request_term(table);
    let resource_request_hash = hash_term(&resource_request);
    let resource_value = resource_authority
        .apply(&mut context, Value::data(resource_request))
        .map_err(|error| authority_error(format!("resource authority apply failed: {error}")))?;
    let authorized_resources = resource::decode_result(
        plain_authority_result(resource_value, &context, "resource")?,
        resource_request_hash,
    )?;
    if authorized_resources.policy_term != resource::legacy_policy_term(policy)? {
        return Err(authority_error(
            "resource result contradicts independently reconstructed log/runtime/store/task policy",
        ));
    }

    let inventory_request = inventory_request_term(&baseline, &override_ops);
    let inventory_request_hash = hash_term(&inventory_request);
    let inventory_value = inventory_authority
        .apply(&mut context, Value::data(inventory_request))
        .map_err(|error| authority_error(format!("inventory authority apply failed: {error}")))?;
    let candidates = decode_inventory_result(
        plain_authority_result(inventory_value, &context, "inventory")?,
        inventory_request_hash,
    )?;
    let mut legacy_candidates: BTreeSet<String> = baseline.iter().cloned().collect();
    legacy_candidates.extend(override_ops);
    if candidates != legacy_candidates.iter().cloned().collect::<Vec<_>>() {
        return Err(authority_error(
            "inventory result contradicts independently reconstructed candidate operations",
        ));
    }

    let legacy_ops = std::mem::take(&mut policy.ops);
    let mut authorized_ops = BTreeMap::new();
    for op in candidates {
        let request = request_term(&op, &baseline, override_term(overrides.get(&op))?);
        let request_hash = hash_term(&request);
        let value = authority
            .clone()
            .apply(&mut context, Value::data(request))
            .map_err(|error| authority_error(format!("authority apply failed: {error}")))?;
        let term = plain_authority_result(value, &context, "operation")?;
        let (allowed, cap) = decode_result(term, &op, request_hash)?;
        let expected = legacy_ops.get(&op);
        if allowed != expected.is_some()
            || expected.is_some_and(|policy| cap != legacy_cap(&op, policy))
        {
            return Err(authority_error(format!(
                "result for `{op}` contradicts independently reconstructed policy composition"
            )));
        }
        if allowed {
            let mut op_policy = legacy_ops
                .get(&op)
                .cloned()
                .ok_or_else(|| authority_error("authorized op has no host enforcement state"))?;
            op_policy.authorized_cap = Some(cap);
            authorized_ops.insert(op, op_policy);
        }
    }
    policy.ops = authorized_ops;
    policy.task = authorized_resources.task;
    policy.runtime = authorized_resources.runtime;
    policy.log.inline_max_bytes = authorized_resources.log_inline_max_bytes;
    policy.log.max_artifact_bytes_per_run = authorized_resources.log_max_artifact_bytes_per_run;
    policy.log.store_dir = authorized_resources.log_store_dir;
    policy.refs.path = authorized_resources.refs_path;
    policy.store.dir = authorized_resources.store_dir;
    policy.store.max_run_bytes = authorized_resources.store_max_run_bytes;
    Ok(())
}
