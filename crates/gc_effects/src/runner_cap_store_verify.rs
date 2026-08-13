use super::*;
use crate::store::{ArtifactHashObservation, StoreInventoryEntry, StoreInventoryObservation};
use crate::store_authority::StoreVerifyDecision;

const VERIFY_MAX_ENTRIES: usize = 8_192;
const VERIFY_MAX_NAME_BYTES: usize = 2 * 1024 * 1024;
const VERIFY_MAX_ARTIFACT_BYTES: usize = HARD_REMOTE_ARTIFACT_MAX_BYTES;
const VERIFY_MAX_TOTAL_BYTES: usize = 512 * 1024 * 1024;

pub(super) fn cap_store_verify(
    op: &str,
    payload: &Term,
    store: Option<&ArtifactStore>,
    authority: Option<&mut StoreAuthority>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let Some(authority) = authority else {
        #[cfg(any(test, feature = "parity-oracle"))]
        {
            return runner_cap_store::cap_store_verify_parity(op, payload, store, error_tok);
        }
        #[cfg(not(any(test, feature = "parity-oracle")))]
        {
            return Err(EffectsError::Log(
                "core/store::verify requires the artifact-loaded GenesisCode store authority"
                    .to_string(),
            ));
        }
    };
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/store::verify".to_string())
    })?;
    let initial = authority.verify(
        payload,
        ":plan",
        None,
        None,
        None,
        VERIFY_MAX_ENTRIES,
        VERIFY_MAX_ARTIFACT_BYTES,
        VERIFY_MAX_TOTAL_BYTES,
    )?;
    let (entries, hashes, single) = match initial {
        StoreVerifyDecision::ObserveInventory => {
            let inventory = match store.observe_inventory(VERIFY_MAX_ENTRIES, VERIFY_MAX_NAME_BYTES)
            {
                Ok(StoreInventoryObservation::Entries(entries)) => entries,
                Ok(StoreInventoryObservation::ResourceLimit) => {
                    let decision = authority.verify(
                        payload,
                        ":inventory-resource-limit",
                        None,
                        None,
                        None,
                        VERIFY_MAX_ENTRIES,
                        VERIFY_MAX_ARTIFACT_BYTES,
                        VERIFY_MAX_TOTAL_BYTES,
                    )?;
                    return finish_verify(op, decision, &[], false, error_tok);
                }
                Err(_error) => {
                    let decision = authority.verify(
                        payload,
                        ":inventory-error",
                        None,
                        None,
                        None,
                        VERIFY_MAX_ENTRIES,
                        VERIFY_MAX_ARTIFACT_BYTES,
                        VERIFY_MAX_TOTAL_BYTES,
                    )?;
                    return finish_verify(op, decision, &[], false, error_tok);
                }
            };
            let entries_term = inventory_term(&inventory);
            let decision = authority.verify(
                payload,
                ":inventory",
                Some(entries_term.clone()),
                None,
                None,
                VERIFY_MAX_ENTRIES,
                VERIFY_MAX_ARTIFACT_BYTES,
                VERIFY_MAX_TOTAL_BYTES,
            )?;
            match decision {
                StoreVerifyDecision::ObserveHashes { hashes } => {
                    (Some(entries_term), hashes, false)
                }
                StoreVerifyDecision::Error { .. } => {
                    return finish_verify(op, decision, &[], false, error_tok);
                }
                _ => return Err(verify_protocol("inventory", ":observe-hashes")),
            }
        }
        StoreVerifyDecision::ObserveHashes { hashes } => (None, hashes, true),
        StoreVerifyDecision::Error { .. } => {
            return finish_verify(op, initial, &[], false, error_tok);
        }
        _ => return Err(verify_protocol("plan", "an observation action")),
    };

    let mut observations = Vec::with_capacity(hashes.len());
    let mut total = 0_usize;
    for hash in &hashes {
        let remaining = VERIFY_MAX_TOTAL_BYTES.saturating_sub(total);
        let limit = VERIFY_MAX_ARTIFACT_BYTES.min(remaining);
        let (status, observed_bytes, observed_hash) = match store.observe_hash_limited(hash, limit)
        {
            Ok(ArtifactHashObservation::Hash { bytes, hash }) => {
                total = total.saturating_add(bytes);
                (":present", Some(bytes), Some(hash))
            }
            Ok(ArtifactHashObservation::Missing) => (":missing", None, None),
            Ok(ArtifactHashObservation::TooLarge) => (":resource-limit", None, None),
            Err(_error) => (":io-error", None, None),
        };
        observations.push(observation_term(
            hash,
            status,
            observed_bytes,
            observed_hash.as_deref(),
        ));
    }
    let decision = authority.verify(
        payload,
        ":observed",
        entries,
        Some(&hashes),
        Some(Term::Vector(observations)),
        VERIFY_MAX_ENTRIES,
        VERIFY_MAX_ARTIFACT_BYTES,
        VERIFY_MAX_TOTAL_BYTES,
    )?;
    finish_verify(op, decision, &hashes, single, error_tok)
}

fn inventory_term(entries: &[StoreInventoryEntry]) -> Term {
    Term::Vector(
        entries
            .iter()
            .map(|entry| {
                Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":kind")), Term::symbol(entry.kind)),
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Bytes(entry.name.clone().into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    )
}

fn observation_term(
    hash: &str,
    status: &str,
    observed_bytes: Option<usize>,
    observed_hash: Option<&str>,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":hash")),
                Term::Str(hash.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":observed-bytes")),
                observed_bytes
                    .map(|value| Term::Int(value.into()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":observed-hash")),
                observed_hash
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (TermOrdKey(Term::symbol(":status")), Term::symbol(status)),
        ]
        .into_iter()
        .collect(),
    )
}

fn finish_verify(
    op: &str,
    decision: StoreVerifyDecision,
    hashes: &[String],
    single: bool,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    match decision {
        StoreVerifyDecision::Return { checked, hash } => {
            let expected_hash = if single {
                Some(
                    hashes
                        .first()
                        .ok_or_else(|| verify_protocol("return", "one specific hash"))?,
                )
            } else {
                None
            };
            if checked != hashes.len() || hash.as_ref() != expected_hash {
                return Err(verify_protocol(
                    "return",
                    "exact observed inventory binding",
                ));
            }
            let mut out = BTreeMap::from([
                (
                    TermOrdKey(Term::symbol(":checked")),
                    Term::Int(checked.into()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            ]);
            if let Some(hash) = hash {
                out.insert(TermOrdKey(Term::symbol(":hash")), Term::Str(hash));
            }
            Ok(Value::data(Term::Map(out)))
        }
        StoreVerifyDecision::Error {
            code,
            message,
            hash,
            checked,
        } => {
            if checked > hashes.len() {
                return Err(verify_protocol("error", "checked count within inventory"));
            }
            if let Some(hash) = &hash
                && checked > 0
                && hashes.get(checked - 1) != Some(hash)
            {
                return Err(verify_protocol("error", "failure hash at checked index"));
            }
            let context = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":checked")),
                        Term::Int(checked.into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":hash")),
                        hash.map(Term::Str).unwrap_or(Term::Nil),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            Ok(mk_error_with_ctx(
                error_tok,
                &code,
                message,
                Some(op),
                context,
            ))
        }
        _ => Err(verify_protocol("final", ":return or :error")),
    }
}

fn verify_protocol(stage: &str, expected: &str) -> EffectsError {
    EffectsError::Log(format!(
        "selfhost store verify authority: {stage} must preserve {expected}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specific_return_without_a_hash_fails_closed_without_panicking() {
        let decision = StoreVerifyDecision::Return {
            checked: 0,
            hash: None,
        };
        let error = finish_verify("core/store::verify", decision, &[], true, SealId(1))
            .expect_err("empty specific result must fail closed");
        assert!(error.to_string().contains("one specific hash"));
    }
}
