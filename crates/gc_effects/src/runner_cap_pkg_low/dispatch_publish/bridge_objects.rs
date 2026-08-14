use super::*;
use crate::pkg_lock_read_authority::{
    PkgBridgeDecision, PkgBridgeFacts, PkgBridgeObject, PkgLockReadAuthority,
};

#[expect(
    clippy::too_many_arguments,
    reason = "bridge dispatch keeps authority, policy, storage, crypto, and diagnostic boundaries explicit"
)]
pub(super) fn dispatch_bridge(
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    authority: Option<&mut PkgLockReadAuthority>,
    budget: &mut ArtifactBudgetState,
    bridge_runtime: &mut HostBridgeRuntime,
    error_tok: SealId,
    op: &str,
) -> Result<Value, EffectsError> {
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/pkg-low::bridge".to_string())
    })?;
    macro_rules! payload_value {
        ($expression:expr) => {
            match $expression {
                Ok(value) => value,
                Err(message) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        message,
                        Some(op),
                    ));
                }
            }
        };
    }
    macro_rules! put {
        ($object:expr) => {
            match put_object(store, $object, policy, budget, error_tok, op)? {
                Ok(hash) => hash,
                Err(error) => return Ok(error),
            }
        };
    }
    let ecosystem = payload_value!(payload_pkg_bridge_ecosystem(payload));
    let name = payload_value!(payload_pkg_name(payload));
    let version = payload_value!(payload_pkg_bridge_version(payload));
    let source = payload_value!(payload_pkg_bridge_source(payload));
    let source_hash = payload_value!(payload_pkg_bridge_source_hash(payload));
    let key_id = payload_value!(payload_pkg_bridge_key_id(payload));
    let public_key_hex = payload_value!(payload_pkg_bridge_public_key(payload));
    let lock_path = payload_value!(payload_pkg_bridge_lock(payload));
    let dep_name = payload_value!(payload_pkg_bridge_dep_name(payload));
    let registry_alias = payload_pkg_registry(payload);

    if lock_path.is_some() != dep_name.is_some() {
        let message = if lock_path.is_some() {
            "bridge lock updates require :dep-name when :lock is provided"
        } else {
            "bridge :dep-name requires :lock"
        };
        return Ok(mk_error(
            error_tok,
            "core/pkg/bad-payload",
            message.to_string(),
            Some(op),
        ));
    }
    let authority = authority.ok_or_else(|| {
        EffectsError::Log(
            "core/pkg-low::bridge requires the artifact-loaded GenesisCode bridge authority"
                .to_string(),
        )
    })?;
    let facts = PkgBridgeFacts {
        ecosystem: &ecosystem,
        name: &name,
        source: &source,
        source_hash: &source_hash,
        version: &version,
    };
    let plan = match authority.plan_bridge(facts)? {
        PkgBridgeDecision::Accept(plan) => plan,
        PkgBridgeDecision::Error { code, message } => {
            return Ok(mk_error(error_tok, &code, message, Some(op)));
        }
    };

    let provenance_root = put!(&plan.provenance);
    let _conversion_data = put!(&plan.conversion_data);
    let conversion_evidence = put!(&plan.conversion_evidence);
    let patch_h = put!(&plan.patch);
    let snapshot_h = put!(&plan.snapshot);

    let public_key = gc_vcs::hex_to_bytes32(&public_key_hex)
        .map_err(|error| EffectsError::BadPayload(format!(":public-key: {error}")))?;
    let sign_payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":algorithm")),
                Term::Str("ed25519".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":key-id")),
                Term::Str(key_id.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":message")),
                Term::Bytes(plan.sign_message.clone().into()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let sign_pol = policy.op_policy("core/crypto::sign").or(pol);
    let sign_out = call_capability_with_runtime(
        "core/crypto::sign",
        &sign_payload,
        sign_pol,
        policy,
        Some(store),
        refs,
        None,
        None,
        None,
        None,
        budget,
        None,
        bridge_runtime,
        error_tok,
    )?;
    let signature = match decode_signature(sign_out, error_tok, op)? {
        Ok(signature) => signature,
        Err(error) => return Ok(error),
    };
    let signature_valid = ed25519_dalek::VerifyingKey::from_bytes(&public_key)
        .map(|key| {
            key.verify_strict(
                &plan.sign_message,
                &ed25519_dalek::Signature::from_bytes(&signature),
            )
            .is_ok()
        })
        .unwrap_or(false);
    let finalized =
        match authority.finalize_bridge(facts, &plan, public_key, signature, signature_valid)? {
            PkgBridgeDecision::Accept(finalized) => finalized,
            PkgBridgeDecision::Error { code, message } => {
                return Ok(mk_error(error_tok, &code, message, Some(op)));
            }
        };
    let attestation_h = put!(&finalized.attestation);
    let commit_h = put!(&finalized.commit);

    let mut lock_h = None;
    if let (Some(lock), Some(dep)) = (lock_path.as_deref(), dep_name.as_deref()) {
        let update = bridge_lock::BridgeLockUpdate {
            lock,
            dep,
            registry: registry_alias.as_deref(),
            commit: &commit_h,
            snapshot: &snapshot_h,
            provenance_root: &provenance_root,
            conversion_evidence: &conversion_evidence,
            attestation: &attestation_h,
        };
        lock_h = Some(
            match bridge_lock::update_lock(update, pol, Some(authority), error_tok, op)? {
                Ok(lock_hash) => lock_hash,
                Err(error) => return Ok(error),
            },
        );
    }

    let mut out = BTreeMap::new();
    for (key, value) in [
        (":attestation", Term::Str(attestation_h)),
        (":commit", Term::Str(commit_h)),
        (":conversion-evidence", Term::Str(conversion_evidence)),
        (":dep-name", dep_name.map(Term::Str).unwrap_or(Term::Nil)),
        (":ecosystem", Term::Str(ecosystem)),
        (":lock-h", lock_h.map(Term::Str).unwrap_or(Term::Nil)),
        (":name", Term::Str(name)),
        (":ok", Term::Bool(true)),
        (":patch", Term::Str(patch_h)),
        (":provenance-root", Term::Str(provenance_root)),
        (
            ":registry",
            registry_alias.map(Term::Str).unwrap_or(Term::Nil),
        ),
        (":snapshot", Term::Str(snapshot_h)),
        (":source", Term::Str(source)),
        (":source-hash", Term::Str(source_hash)),
        (":version", Term::Str(version)),
    ] {
        out.insert(TermOrdKey(Term::symbol(key)), value);
    }
    Ok(Value::data(Term::Map(out)))
}

fn put_object(
    store: &ArtifactStore,
    object: &PkgBridgeObject,
    policy: &CapsPolicy,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
) -> Result<Result<String, Value>, EffectsError> {
    let stored = match store_put_with_budget(store, &object.bytes, policy, budget, error_tok, op) {
        Ok(stored) => stored,
        Err(error) => return Ok(Err(error)),
    };
    if stored != object.hash {
        return Err(EffectsError::Log(format!(
            "bridge store identity contradiction: authority={} store={stored}",
            object.hash
        )));
    }
    Ok(Ok(stored))
}

fn decode_signature(
    value: Value,
    error_tok: SealId,
    op: &str,
) -> Result<Result<[u8; 64], Value>, EffectsError> {
    match value {
        Value::Sealed { .. } => Ok(Err(value)),
        Value::Data(term) => {
            let Term::Map(fields) = term.as_ref() else {
                return Ok(Err(mk_error(
                    error_tok,
                    "core/pkg/bridge-signature",
                    "core/crypto::sign response must be a map".to_string(),
                    Some(op),
                )));
            };
            match fields.get(&TermOrdKey(Term::symbol(":signature"))) {
                Some(Term::Bytes(bytes)) => match bytes.as_ref().try_into() {
                    Ok(signature) => Ok(Ok(signature)),
                    Err(_) => Ok(Err(mk_error(
                        error_tok,
                        "core/pkg/bridge-signature",
                        format!("signature must be 64 bytes, got {}", bytes.len()),
                        Some(op),
                    ))),
                },
                Some(other) => Ok(Err(mk_error(
                    error_tok,
                    "core/pkg/bridge-signature",
                    format!("signature must be bytes, got {}", print_term(other)),
                    Some(op),
                ))),
                None => Ok(Err(mk_error(
                    error_tok,
                    "core/pkg/bridge-signature",
                    "core/crypto::sign response missing :signature".to_string(),
                    Some(op),
                ))),
            }
        }
        other => Ok(Err(mk_error(
            error_tok,
            "core/pkg/bridge-signature",
            format!(
                "unexpected core/crypto::sign response: {}",
                other.debug_repr()
            ),
            Some(op),
        ))),
    }
}
