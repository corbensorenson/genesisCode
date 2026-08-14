use super::*;

fn failure(
    status: PkgVerifyClosureStatus,
    hash: Option<String>,
    detail: Option<String>,
) -> PkgVerifyClosureObservation {
    PkgVerifyClosureObservation {
        checked: 0,
        detail,
        hash,
        status,
    }
}

pub(super) fn observe_verify_commit_closure(
    store: &ArtifactStore,
    snapshot_hex: &str,
    commit_hex: &str,
) -> PkgVerifyClosureObservation {
    let mut checked = 0_u64;
    let mut seen = std::collections::BTreeSet::new();
    let mut ensure_hash = |hash: &str| -> Result<(), PkgVerifyClosureObservation> {
        if !store.path_for(hash).exists() {
            return Err(failure(
                PkgVerifyClosureStatus::Missing,
                Some(hash.to_string()),
                None,
            ));
        }
        if store.verify_hex(hash).is_err() {
            return Err(failure(
                PkgVerifyClosureStatus::Corrupt,
                Some(hash.to_string()),
                None,
            ));
        }
        if seen.insert(hash.to_string()) {
            checked = checked.saturating_add(1);
        }
        Ok(())
    };
    if let Err(observation) = ensure_hash(commit_hex) {
        return observation;
    }
    let commit_term = match store_get_term(store, commit_hex) {
        Ok(term) => term,
        Err(_) => {
            return failure(
                PkgVerifyClosureStatus::Missing,
                Some(commit_hex.to_string()),
                None,
            );
        }
    };
    let commit = match gc_vcs::Commit::from_term(&commit_term) {
        Ok(commit) => commit,
        Err(error) => {
            return failure(
                PkgVerifyClosureStatus::BadCommit,
                None,
                Some(error.to_string()),
            );
        }
    };
    if commit.result != snapshot_hex {
        return failure(PkgVerifyClosureStatus::SnapshotMismatch, None, None);
    }
    if let Some(base) = commit.base.as_deref()
        && let Err(observation) = ensure_hash(base)
    {
        return observation;
    }
    if let Err(observation) = ensure_hash(&commit.patch) {
        return observation;
    }
    if let Err(observation) = ensure_hash(&commit.result) {
        return observation;
    }
    if !commit.obligations.is_empty() && commit.evidence.is_empty() {
        return failure(PkgVerifyClosureStatus::MissingEvidence, None, None);
    }
    for evidence_hash in &commit.evidence {
        if let Err(observation) = ensure_hash(evidence_hash) {
            return observation;
        }
        let evidence_term = match store_get_term(store, evidence_hash) {
            Ok(term) => term,
            Err(error) => {
                return failure(
                    PkgVerifyClosureStatus::BadEvidence,
                    None,
                    Some(error.to_string()),
                );
            }
        };
        if let Err(error) = gc_vcs::Evidence::from_term(&evidence_term) {
            return failure(
                PkgVerifyClosureStatus::BadEvidence,
                None,
                Some(error.to_string()),
            );
        }
    }
    for attestation_hash in &commit.attestations {
        if let Err(observation) = ensure_hash(attestation_hash) {
            return observation;
        }
        let attestation_term = match store_get_term(store, attestation_hash) {
            Ok(term) => term,
            Err(error) => {
                return failure(
                    PkgVerifyClosureStatus::BadAttestation,
                    None,
                    Some(error.to_string()),
                );
            }
        };
        if let Err(error) = gc_vcs::Attestation::from_term(&attestation_term) {
            return failure(
                PkgVerifyClosureStatus::BadAttestation,
                None,
                Some(error.to_string()),
            );
        }
    }
    PkgVerifyClosureObservation {
        checked,
        detail: None,
        hash: None,
        status: PkgVerifyClosureStatus::Ok,
    }
}
