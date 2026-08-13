use super::*;
use crate::store::ArtifactObservation;
use crate::store_authority::{StoreGetDecision, StoreHasDecision};

#[expect(
    clippy::too_many_arguments,
    reason = "store read path receives explicit policy, mechanism, and authority state"
)]
pub(super) fn cap_store_has(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    authority: Option<&mut StoreAuthority>,
    timeout_ms: Option<u64>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let Some(authority) = authority else {
        #[cfg(any(test, feature = "parity-oracle"))]
        {
            return runner_cap_store::cap_store_has_parity(
                op, payload, pol, policy, store, timeout_ms, error_tok,
            );
        }
        #[cfg(not(any(test, feature = "parity-oracle")))]
        {
            return Err(missing_authority(op));
        }
    };
    let store =
        store.ok_or_else(|| EffectsError::Log(format!("missing artifact store for {op}")))?;
    let hash = match authority.has(payload, ":plan", false, None, None, None)? {
        StoreHasDecision::ObserveLocal { hash } => hash,
        StoreHasDecision::Error { code, message } => {
            return Ok(mk_error(error_tok, &code, message, Some(op)));
        }
        _ => return Err(protocol_order("has plan", ":observe-local")),
    };

    let observation = store.observe_bytes_limited(&hash, HARD_REMOTE_ARTIFACT_MAX_BYTES);
    let decision = match observation {
        Ok(ArtifactObservation::Bytes(bytes)) => authority.has(
            payload,
            ":local-present",
            false,
            Some(&hash_bytes_hex(&bytes)),
            None,
            None,
        )?,
        Ok(ArtifactObservation::TooLarge { observed }) => authority.has(
            payload,
            ":local-resource-limit",
            false,
            None,
            None,
            Some(&format!(
                "local artifact bytes {observed} exceed {}",
                HARD_REMOTE_ARTIFACT_MAX_BYTES
            )),
        )?,
        Err(_error) => authority.has(
            payload,
            ":local-io-error",
            false,
            None,
            None,
            Some("artifact store read failed"),
        )?,
        Ok(ArtifactObservation::Missing) => {
            let remote = match store_remote_client(policy, pol, timeout_ms, error_tok, op) {
                Ok(remote) => remote,
                Err(value) => return Ok(value),
            };
            let decision = authority.has(
                payload,
                ":local-missing",
                remote.is_some(),
                None,
                None,
                None,
            )?;
            match decision {
                StoreHasDecision::FetchRemote { hash: remote_hash } => {
                    if remote_hash != hash {
                        return Err(protocol_order("has remote plan", "request-bound hash"));
                    }
                    let Some((client, _)) = remote else {
                        return Err(protocol_order("has remote plan", "configured client"));
                    };
                    match client.store_has(std::slice::from_ref(&hash)) {
                        Ok(present) => authority.has(
                            payload,
                            ":remote-present",
                            true,
                            None,
                            Some(present.get(&hash).copied().unwrap_or(false)),
                            None,
                        )?,
                        Err(error) => {
                            let (status, message) =
                                if matches!(error, gc_registry::RegistryError::Auth(_)) {
                                    (
                                        ":remote-auth-error",
                                        "remote artifact store authentication failed",
                                    )
                                } else {
                                    (":remote-error", "remote artifact store request failed")
                                };
                            authority.has(payload, status, true, None, None, Some(message))?
                        }
                    }
                }
                other => other,
            }
        }
    };
    finish_has(op, decision, error_tok)
}

#[expect(
    clippy::too_many_arguments,
    reason = "store read path receives explicit policy, budget, mechanism, and authority state"
)]
pub(super) fn cap_store_get(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    budget: &mut ArtifactBudgetState,
    authority: Option<&mut StoreAuthority>,
    timeout_ms: Option<u64>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let Some(authority) = authority else {
        #[cfg(any(test, feature = "parity-oracle"))]
        {
            return runner_cap_store::cap_store_get_parity(
                op, payload, pol, policy, store, budget, timeout_ms, error_tok,
            );
        }
        #[cfg(not(any(test, feature = "parity-oracle")))]
        {
            return Err(missing_authority(op));
        }
    };
    let store =
        store.ok_or_else(|| EffectsError::Log(format!("missing artifact store for {op}")))?;
    let configured_max = match op_extra_positive_usize(pol, "max_bytes") {
        Ok(value) => value,
        Err(error) => {
            return Ok(mk_error(
                error_tok,
                "core/caps/policy-error",
                error,
                Some(op),
            ));
        }
    };
    let max_bytes = effective_limit(configured_max, HARD_REMOTE_ARTIFACT_MAX_BYTES);
    let mut decide =
        |status: &str, bytes: Option<&[u8]>, remote_enabled: bool, message: Option<&str>| {
            authority.get(
                payload,
                status,
                bytes,
                remote_enabled,
                message,
                max_bytes,
                budget.store_written_bytes,
                policy.store.max_run_bytes,
            )
        };
    let hash = match decide(":plan", None, false, None)? {
        StoreGetDecision::ObserveLocal { hash } => hash,
        StoreGetDecision::Error { code, message } => {
            return Ok(mk_error(error_tok, &code, message, Some(op)));
        }
        _ => return Err(protocol_order("get plan", ":observe-local")),
    };

    let decision = match store.observe_bytes_limited(&hash, max_bytes) {
        Ok(ArtifactObservation::Bytes(bytes)) => decide(":local-found", Some(&bytes), false, None)?,
        Ok(ArtifactObservation::TooLarge { .. }) => {
            decide(":local-resource-limit", None, false, None)?
        }
        Err(_error) => decide(
            ":local-io-error",
            None,
            false,
            Some("artifact store read failed"),
        )?,
        Ok(ArtifactObservation::Missing) => {
            let remote = match store_remote_client(policy, pol, timeout_ms, error_tok, op) {
                Ok(remote) => remote,
                Err(value) => return Ok(value),
            };
            let decision = decide(":local-missing", None, remote.is_some(), None)?;
            match decision {
                StoreGetDecision::FetchRemote { hash: remote_hash } => {
                    if remote_hash != hash {
                        return Err(protocol_order("get remote plan", "request-bound hash"));
                    }
                    let Some((client, _)) = remote else {
                        return Err(protocol_order("get remote plan", "configured client"));
                    };
                    match client.store_get_opt_bounded(&hash, Some(max_bytes)) {
                        Ok(Some(bytes)) => decide(":remote-found", Some(&bytes), true, None)?,
                        Ok(None) => decide(":remote-not-found", None, true, None)?,
                        Err(error) => {
                            let rendered = error.to_string();
                            let status = if matches!(
                                error,
                                gc_registry::RegistryError::Protocol(ref message)
                                    if message == "store/get: hash mismatch"
                            ) {
                                ":remote-hash-mismatch"
                            } else if rendered.contains("resource-limit:") {
                                ":remote-resource-limit"
                            } else if matches!(error, gc_registry::RegistryError::Auth(_)) {
                                ":remote-auth-error"
                            } else {
                                ":remote-error"
                            };
                            let message = match status {
                                ":remote-hash-mismatch" => "remote artifact hash mismatch",
                                ":remote-resource-limit" => {
                                    "remote artifact exceeds configured limit"
                                }
                                ":remote-auth-error" => {
                                    "remote artifact store authentication failed"
                                }
                                _ => "remote artifact store request failed",
                            };
                            decide(status, None, true, Some(message))?
                        }
                    }
                }
                other => other,
            }
        }
    };
    finish_get(op, store, budget, &hash, decision, error_tok)
}

fn finish_has(
    op: &str,
    decision: StoreHasDecision,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    match decision {
        StoreHasDecision::Return { present } => Ok(Value::data(Term::Map(
            [(TermOrdKey(Term::symbol(":present")), Term::Bool(present))]
                .into_iter()
                .collect(),
        ))),
        StoreHasDecision::Error { code, message } => {
            Ok(mk_error(error_tok, &code, message, Some(op)))
        }
        _ => Err(protocol_order("has final", ":return or :error")),
    }
}

fn finish_get(
    op: &str,
    store: &ArtifactStore,
    budget: &mut ArtifactBudgetState,
    expected_hash: &str,
    decision: StoreGetDecision,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let artifact = match decision {
        StoreGetDecision::Return { artifact, hash } => {
            if hash != expected_hash {
                return Err(protocol_order("get return", "planner-approved hash"));
            }
            artifact
        }
        StoreGetDecision::CacheReturn {
            artifact,
            bytes,
            hash,
            written_bytes,
        } => {
            if hash != expected_hash {
                return Err(protocol_order("get cache return", "planner-approved hash"));
            }
            let stored_hash = match store.put_bytes(&bytes) {
                Ok(hash) => hash,
                Err(_error) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/io-error",
                        "artifact store cache write failed".to_string(),
                        Some(op),
                    ));
                }
            };
            if stored_hash != hash {
                return Err(protocol_order("get cache write", "authority-approved hash"));
            }
            budget.store_written_bytes = budget.store_written_bytes.saturating_add(written_bytes);
            artifact
        }
        StoreGetDecision::Error { code, message } => {
            return Ok(mk_error(error_tok, &code, message, Some(op)));
        }
        _ => {
            return Err(protocol_order(
                "get final",
                ":return, :cache-return, or :error",
            ));
        }
    };
    Ok(Value::data(Term::Map(
        [(TermOrdKey(Term::symbol(":artifact")), artifact)]
            .into_iter()
            .collect(),
    )))
}

fn missing_authority(op: &str) -> EffectsError {
    EffectsError::Log(format!(
        "{op} requires the artifact-loaded GenesisCode store authority"
    ))
}

fn protocol_order(stage: &str, expected: &str) -> EffectsError {
    EffectsError::Log(format!(
        "selfhost store authority: {stage} must return {expected}"
    ))
}
