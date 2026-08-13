use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};
use num_traits::ToPrimitive;

use crate::error::EffectsError;

use super::{
    AuthorizedBridgeIdentityPolicy, AuthorizedCryptoPolicy, AuthorizedDatabasePolicy,
    AuthorizedFfiPolicy, AuthorizedGfxProfile, AuthorizedGpuPolicy, AuthorizedMaxBytes,
    AuthorizedNetworkPolicy, AuthorizedPluginPolicy, AuthorizedProcessPrograms,
    AuthorizedStoreCredentials, AuthorizedStoreRemotePolicy, AuthorizedXrPolicy, CapsPolicy,
    OpPolicy,
};

#[path = "policy_authority_bridge.rs"]
mod bridge;
#[path = "policy_authority_cap.rs"]
mod cap;
#[path = "policy_authority_crypto.rs"]
mod crypto;
#[path = "policy_authority_database.rs"]
mod database;
#[path = "policy_authority_ffi.rs"]
mod ffi;
#[path = "policy_authority_gfx.rs"]
pub(super) mod gfx;
#[path = "policy_authority_gpu.rs"]
pub(super) mod gpu;
#[path = "policy_authority_limit.rs"]
mod limit;
#[path = "policy_authority_network.rs"]
mod network;
#[path = "policy_authority_override.rs"]
mod operation_override;
#[path = "policy_authority_plugin.rs"]
mod plugin;
#[path = "policy_authority_process.rs"]
mod process;
#[path = "policy_authority_request.rs"]
mod request;
#[path = "policy_authority_resource.rs"]
mod resource;
#[path = "policy_authority_store_credentials.rs"]
pub(super) mod store_credentials;
#[path = "policy_authority_store_remote.rs"]
mod store_remote;
#[path = "policy_authority_xr.rs"]
pub(super) mod xr;
pub(super) fn legacy_ffi_policy(policy: Option<&OpPolicy>) -> AuthorizedFfiPolicy {
    ffi::legacy(policy)
}
pub(super) fn legacy_bridge_identity_policy(
    op: &str,
    policy: Option<&OpPolicy>,
) -> AuthorizedBridgeIdentityPolicy {
    bridge::legacy(op, policy)
}
#[cfg(test)]
pub(super) fn decode_bridge_identity_policy(
    term: &Term,
    op: &str,
    allowed: bool,
) -> Result<AuthorizedBridgeIdentityPolicy, EffectsError> {
    bridge::decode(term, op, allowed)
}
pub(super) fn legacy_store_remote_policy(
    store: Option<&toml::value::Table>,
) -> AuthorizedStoreRemotePolicy {
    store_remote::legacy(store)
}
#[cfg(test)]
pub(super) fn decode_store_remote_policy(
    term: &Term,
) -> Result<AuthorizedStoreRemotePolicy, EffectsError> {
    store_remote::decode(term)
}

pub(super) fn legacy_store_credentials_policy(
    store: Option<&toml::value::Table>,
    raw: &super::StorePolicy,
) -> super::AuthorizedStoreCredentials {
    store_credentials::legacy(store, raw)
}

pub(super) fn legacy_sync_credentials_policy(
    operation: Option<&toml::value::Table>,
) -> super::AuthorizedStoreCredentials {
    store_credentials::legacy_operation(operation)
}

#[cfg(test)]
pub(super) fn decode_process_program_policy(
    term: &Term,
    allowed: bool,
) -> Result<AuthorizedProcessPrograms, EffectsError> {
    process::decode(term, allowed)
}
#[cfg(test)]
pub(super) fn decode_database_policy(
    term: &Term,
    allowed: bool,
) -> Result<AuthorizedDatabasePolicy, EffectsError> {
    database::decode(term, allowed)
}
#[cfg(test)]
pub(super) fn decode_network_policy(
    term: &Term,
    allowed: bool,
) -> Result<AuthorizedNetworkPolicy, EffectsError> {
    network::decode(term, allowed)
}
#[cfg(test)]
pub(super) fn decode_crypto_policy(
    term: &Term,
    allowed: bool,
) -> Result<AuthorizedCryptoPolicy, EffectsError> {
    crypto::decode(term, allowed)
}
#[cfg(test)]
pub(super) fn decode_ffi_policy(
    term: &Term,
    allowed: bool,
) -> Result<AuthorizedFfiPolicy, EffectsError> {
    ffi::decode(term, allowed)
}
#[cfg(test)]
pub(super) fn decode_plugin_policy(
    term: &Term,
    allowed: bool,
) -> Result<AuthorizedPluginPolicy, EffectsError> {
    plugin::decode(term, allowed)
}
#[cfg(test)]
pub(super) fn decode_cap(
    term: &Term,
    op: &str,
) -> Result<(bool, Option<u64>, Option<usize>), EffectsError> {
    cap::decode(term, op)
}

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

fn max_bytes_input(value: Option<&toml::Value>) -> Term {
    match value {
        None => Term::Nil,
        Some(value) => value
            .as_integer()
            .map(|number| Term::Int(number.into()))
            .unwrap_or_else(|| Term::symbol(":invalid-type")),
    }
}

pub(super) fn decode_max_bytes_policy(
    term: &Term,
    allowed: bool,
) -> Result<AuthorizedMaxBytes, EffectsError> {
    limit::decode(term, allowed)
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
    candidate_domain: &BTreeSet<String>,
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
    if let Some(invented) = candidates
        .iter()
        .find(|candidate| !candidate_domain.contains(*candidate))
    {
        return Err(authority_error(format!(
            "inventory result invented operation `{invented}` outside the transported candidate domain"
        )));
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

struct AuthorizedOperation {
    allowed: bool,
    base_dir: Option<PathBuf>,
    create_dirs: bool,
    timeout_ms: Option<u64>,
    log_inline_max_bytes: Option<usize>,
    max_bytes: AuthorizedMaxBytes,
    bridge_identity: AuthorizedBridgeIdentityPolicy,
    process_programs: AuthorizedProcessPrograms,
    database: AuthorizedDatabasePolicy,
    network: AuthorizedNetworkPolicy,
    crypto: AuthorizedCryptoPolicy,
    credentials: AuthorizedStoreCredentials,
    gfx: AuthorizedGfxProfile,
    gpu: AuthorizedGpuPolicy,
    xr: AuthorizedXrPolicy,
    ffi: AuthorizedFfiPolicy,
    plugin: AuthorizedPluginPolicy,
    cap: Term,
}

fn decode_result(
    term: Term,
    op: &str,
    request_hash: [u8; 32],
    override_table: Option<&toml::value::Table>,
) -> Result<AuthorizedOperation, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error("result must be a data map"));
    };
    let expected_keys: BTreeSet<_> = [
        ":allowed",
        ":base-dir",
        ":bridge-identity-policy",
        ":cap",
        ":crypto-policy",
        ":credential-policy",
        ":database-policy",
        ":ffi-policy",
        ":gfx-policy",
        ":gpu-policy",
        ":kind",
        ":max-bytes-policy",
        ":network-policy",
        ":op",
        ":plugin-policy",
        ":process-program-policy",
        ":request-h",
        ":v",
        ":xr-policy",
    ]
    .into_iter()
    .map(|key| TermOrdKey(Term::symbol(key)))
    .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected_keys {
        return Err(authority_error("result field set mismatch"));
    }
    if !matches!(map.get(&TermOrdKey(Term::symbol(":kind"))), Some(Term::Str(kind)) if kind == "genesis/effect-policy-authority-result-v0.20")
        || !matches!(map.get(&TermOrdKey(Term::symbol(":v"))), Some(Term::Int(version)) if version == &20.into())
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
    let base_dir = match map.get(&TermOrdKey(Term::symbol(":base-dir"))) {
        Some(Term::Nil) => None,
        Some(Term::Str(path)) => Some(PathBuf::from(path)),
        _ => return Err(authority_error("result :base-dir must be nil or string")),
    };
    if !allowed && base_dir.is_some() {
        return Err(authority_error("result :base-dir contradicts :allowed"));
    }
    let (create_dirs, timeout_ms, log_inline_max_bytes) = if allowed {
        cap::decode(&cap, op)?
    } else {
        (false, None, None)
    };
    let max_bytes = limit::decode(
        map.get(&TermOrdKey(Term::symbol(":max-bytes-policy")))
            .ok_or_else(|| authority_error("result is missing :max-bytes-policy"))?,
        allowed,
    )?;
    let bridge_identity = bridge::decode(
        map.get(&TermOrdKey(Term::symbol(":bridge-identity-policy")))
            .ok_or_else(|| authority_error("result is missing :bridge-identity-policy"))?,
        op,
        allowed,
    )?;
    let process_programs = process::decode(
        map.get(&TermOrdKey(Term::symbol(":process-program-policy")))
            .ok_or_else(|| authority_error("result is missing :process-program-policy"))?,
        allowed,
    )?;
    let database = database::decode(
        map.get(&TermOrdKey(Term::symbol(":database-policy")))
            .ok_or_else(|| authority_error("result is missing :database-policy"))?,
        allowed,
    )?;
    let network = network::decode(
        map.get(&TermOrdKey(Term::symbol(":network-policy")))
            .ok_or_else(|| authority_error("result is missing :network-policy"))?,
        allowed,
    )?;
    let crypto = crypto::decode(
        map.get(&TermOrdKey(Term::symbol(":crypto-policy")))
            .ok_or_else(|| authority_error("result is missing :crypto-policy"))?,
        allowed,
    )?;
    let credential_term = map
        .get(&TermOrdKey(Term::symbol(":credential-policy")))
        .ok_or_else(|| authority_error("result is missing :credential-policy"))?;
    let credentials = if allowed && store_credentials::operation_applies(op) {
        store_credentials::decode_operation(credential_term, override_table)?
    } else {
        if credential_term != &Term::Nil {
            return Err(authority_error(
                "result :credential-policy contradicts denied operation",
            ));
        }
        store_credentials::legacy_operation(None)
    };
    let gpu = gpu::decode(
        map.get(&TermOrdKey(Term::symbol(":gpu-policy")))
            .ok_or_else(|| authority_error("result is missing :gpu-policy"))?,
        allowed,
    )?;
    let gfx = gfx::decode(
        map.get(&TermOrdKey(Term::symbol(":gfx-policy")))
            .ok_or_else(|| authority_error("result is missing :gfx-policy"))?,
        allowed,
    )?;
    let xr = xr::decode(
        map.get(&TermOrdKey(Term::symbol(":xr-policy")))
            .ok_or_else(|| authority_error("result is missing :xr-policy"))?,
        allowed,
    )?;
    let ffi = ffi::decode(
        map.get(&TermOrdKey(Term::symbol(":ffi-policy")))
            .ok_or_else(|| authority_error("result is missing :ffi-policy"))?,
        allowed,
    )?;
    let plugin = plugin::decode(
        map.get(&TermOrdKey(Term::symbol(":plugin-policy")))
            .ok_or_else(|| authority_error("result is missing :plugin-policy"))?,
        allowed,
    )?;
    Ok(AuthorizedOperation {
        allowed,
        base_dir,
        create_dirs,
        timeout_ms,
        log_inline_max_bytes,
        max_bytes,
        bridge_identity,
        process_programs,
        database,
        network,
        crypto,
        credentials,
        gfx,
        gpu,
        xr,
        ffi,
        plugin,
        cap,
    })
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
        &policy.store,
    )?;
    let inventory_request = inventory_request_term(&baseline, &override_ops);
    let inventory_request_hash = hash_term(&inventory_request);
    let inventory_value = inventory_authority
        .apply(&mut context, Value::data(inventory_request))
        .map_err(|error| authority_error(format!("inventory authority apply failed: {error}")))?;
    let candidate_domain: BTreeSet<String> = policy.ops.keys().cloned().collect();
    let candidates = decode_inventory_result(
        plain_authority_result(inventory_value, &context, "inventory")?,
        inventory_request_hash,
        &candidate_domain,
    )?;

    let candidate_states = std::mem::take(&mut policy.ops);
    let gpu_default = gpu::observed_default();
    let mut authorized_ops = BTreeMap::new();
    for op in candidates {
        let override_table = overrides.get(&op).and_then(toml::Value::as_table);
        let request = request::term(
            &op,
            &baseline,
            operation_override::term(&op, overrides.get(&op))?,
            gfx::input(override_table),
            gpu::input(override_table, gpu_default.as_deref()),
            xr::input(override_table),
        );
        let request_hash = hash_term(&request);
        let value = authority
            .clone()
            .apply(&mut context, Value::data(request))
            .map_err(|error| authority_error(format!("authority apply failed: {error}")))?;
        let term = plain_authority_result(value, &context, "operation")?;
        let authorized = decode_result(term, &op, request_hash, override_table)?;
        if authorized.allowed {
            let mut op_policy = candidate_states.get(&op).cloned().ok_or_else(|| {
                authority_error("authorized op is outside the transported candidate domain")
            })?;
            op_policy.base_dir = authorized.base_dir;
            op_policy.create_dirs = authorized.create_dirs;
            op_policy.timeout_ms = authorized.timeout_ms;
            op_policy.log_inline_max_bytes = authorized.log_inline_max_bytes;
            op_policy.authorized_max_bytes = Some(authorized.max_bytes);
            op_policy.authorized_bridge_identity = Some(authorized.bridge_identity);
            op_policy.authorized_process_programs = Some(authorized.process_programs);
            op_policy.authorized_database = Some(authorized.database);
            op_policy.authorized_network = Some(authorized.network);
            op_policy.authorized_crypto = Some(authorized.crypto);
            op_policy.authorized_sync_credentials = Some(authorized.credentials);
            op_policy.authorized_gfx_profile = Some(authorized.gfx);
            op_policy.authorized_gpu = Some(authorized.gpu);
            op_policy.authorized_xr_policy = Some(authorized.xr);
            op_policy.authorized_ffi = Some(authorized.ffi);
            op_policy.authorized_plugin = Some(authorized.plugin);
            op_policy.authorized_cap = Some(authorized.cap);
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
    policy.store.authorized_remote = Some(authorized_resources.store_remote);
    policy.store.authorized_credentials = Some(authorized_resources.store_credentials);
    Ok(())
}

#[cfg(test)]
mod inventory_tests {
    use super::*;

    fn inventory_result(request_hash: [u8; 32], candidates: &[&str]) -> Term {
        Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":candidate-ops")),
                    Term::Vector(
                        candidates
                            .iter()
                            .map(|candidate| Term::Str((*candidate).to_string()))
                            .collect(),
                    ),
                ),
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/effect-policy-inventory-result-v0.1".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":request-h")),
                    Term::Str(hex32(request_hash)),
                ),
                (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            ]
            .into_iter()
            .collect(),
        )
    }

    #[test]
    fn inventory_decoder_allows_authoritative_denial_by_omission() {
        let hash = [7; 32];
        let domain = ["core/a", "core/b"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let decoded = decode_inventory_result(inventory_result(hash, &["core/a"]), hash, &domain)
            .expect("a strict candidate-domain subset is a valid authoritative inventory");
        assert_eq!(decoded, vec!["core/a"]);
    }

    #[test]
    fn inventory_decoder_rejects_invented_operations() {
        let hash = [9; 32];
        let domain = ["core/a"].into_iter().map(str::to_string).collect();
        let error = decode_inventory_result(
            inventory_result(hash, &["core/a", "core/invented"]),
            hash,
            &domain,
        )
        .expect_err("authority output cannot widen the transported candidate domain");
        assert!(
            error
                .to_string()
                .contains("invented operation `core/invented`")
        );
    }
}
