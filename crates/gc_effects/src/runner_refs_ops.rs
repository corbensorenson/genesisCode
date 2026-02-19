use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};

use crate::EffectsError;
use crate::refs::{RefsDb, SetResult};
use crate::runner_store_ops::store_get_term;
use crate::store::ArtifactStore;

#[derive(Copy, Clone)]
pub(crate) struct LocalRefSetRequest<'a> {
    pub(crate) name: &'a str,
    pub(crate) new_hash: Option<&'a str>,
    pub(crate) expected_old: Option<Option<&'a str>>,
    pub(crate) policy_h: &'a str,
}

pub(crate) fn local_refs_set_policy_gated(
    store: &ArtifactStore,
    refs: &RefsDb,
    req: LocalRefSetRequest<'_>,
    error_tok: SealId,
    op: &str,
) -> Result<SetResult, Value> {
    local_refs_validate_policy_gate(store, req.name, req.new_hash, req.policy_h, error_tok, op)?;
    refs.set(req.name, req.new_hash, req.expected_old)
        .map_err(|e| mk_error(error_tok, "core/refs/io-error", e.to_string(), Some(op)))
}

pub(crate) fn local_refs_validate_policy_gate(
    store: &ArtifactStore,
    name: &str,
    new_hash: Option<&str>,
    policy_h: &str,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    let pol_term = match store_get_term(store, policy_h) {
        Ok(t) => t,
        Err(_) => {
            return Err(mk_error(
                error_tok,
                "core/refs/policy-not-found",
                format!("policy artifact not found: {policy_h}"),
                Some(op),
            ));
        }
    };
    let pol = match gc_vcs::Policy::from_term(&pol_term) {
        Ok(p) => p,
        Err(e) => {
            return Err(mk_error(
                error_tok,
                "core/refs/bad-policy",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    if pol.is_frozen_ref(name) {
        return Err(mk_error(
            error_tok,
            "core/refs/frozen",
            format!("ref is frozen by policy: {name}"),
            Some(op),
        ));
    }
    let class = match pol.class_for_ref(name) {
        Some(c) => c,
        None => {
            return Err(mk_error(
                error_tok,
                "core/refs/no-class",
                format!("policy has no matching class for ref {name}"),
                Some(op),
            ));
        }
    };

    if let Some(h) = new_hash {
        let commit_term = match store_get_term(store, h) {
            Ok(t) => t,
            Err(_) => {
                return Err(mk_error(
                    error_tok,
                    "core/refs/commit-not-found",
                    format!("commit artifact not found: {h}"),
                    Some(op),
                ));
            }
        };
        let commit = match gc_vcs::Commit::from_term(&commit_term) {
            Ok(c) => c,
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/refs/bad-commit",
                    format!("{e}"),
                    Some(op),
                ));
            }
        };

        for req in &class.required_obligations {
            if !commit.obligations.iter().any(|o| o == req) {
                return Err(mk_error(
                    error_tok,
                    "core/refs/missing-obligation",
                    format!("commit missing required obligation: {req}"),
                    Some(op),
                ));
            }
        }
        if !class.required_obligations.is_empty() && commit.evidence.is_empty() {
            return Err(mk_error(
                error_tok,
                "core/refs/missing-evidence",
                "commit has required obligations but no evidence".to_string(),
                Some(op),
            ));
        }
        let mut evidence_kinds: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for ev_h in &commit.evidence {
            if store.path_for(ev_h).exists() {
                if store.verify_hex(ev_h).is_err() {
                    return Err(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("evidence artifact corrupted: {ev_h}"),
                        Some(op),
                    ));
                }
            } else {
                return Err(mk_error(
                    error_tok,
                    "core/store/not-found",
                    format!("evidence artifact not found: {ev_h}"),
                    Some(op),
                ));
            }
            let ev_t = match store_get_term(store, ev_h) {
                Ok(t) => t,
                Err(_) => {
                    return Err(mk_error(
                        error_tok,
                        "core/refs/bad-evidence",
                        format!("evidence artifact is not a valid CoreForm term: {ev_h}"),
                        Some(op),
                    ));
                }
            };
            let ev = match gc_vcs::Evidence::from_term(&ev_t) {
                Ok(ev) => ev,
                Err(e) => {
                    return Err(mk_error(
                        error_tok,
                        "core/refs/bad-evidence",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            evidence_kinds.insert(gc_vcs::PolicyClass::normalize_evidence_kind(&ev.kind));
        }

        let missing_kinds =
            class.missing_required_evidence_kinds(&commit.obligations, &evidence_kinds);
        if !missing_kinds.is_empty() {
            return Err(mk_error(
                error_tok,
                "core/refs/missing-evidence-kind",
                format!(
                    "commit evidence missing required kinds: {}",
                    missing_kinds.join(", ")
                ),
                Some(op),
            ));
        }

        if class.require_signatures {
            let signing_h = match gc_vcs::commit_signing_hash(&commit_term) {
                Ok(hh) => hh,
                Err(e) => {
                    return Err(mk_error(
                        error_tok,
                        "core/refs/bad-commit",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let mut valid: u64 = 0;
            let mut seen_pks: std::collections::BTreeSet<Vec<u8>> =
                std::collections::BTreeSet::new();
            for at_h in &commit.attestations {
                if store.path_for(at_h).exists() {
                    if store.verify_hex(at_h).is_err() {
                        return Err(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("attestation artifact corrupted: {at_h}"),
                            Some(op),
                        ));
                    }
                } else {
                    return Err(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("attestation artifact not found: {at_h}"),
                        Some(op),
                    ));
                }
                let at_t = match store_get_term(store, at_h) {
                    Ok(t) => t,
                    Err(_) => {
                        return Err(mk_error(
                            error_tok,
                            "core/refs/bad-attestation",
                            format!("attestation artifact is not a valid CoreForm term: {at_h}"),
                            Some(op),
                        ));
                    }
                };
                let at = match gc_vcs::Attestation::from_term(&at_t) {
                    Ok(a) => a,
                    Err(e) => {
                        return Err(mk_error(
                            error_tok,
                            "core/refs/bad-attestation",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };
                let pk_vec = at.pk.to_vec();
                if seen_pks.contains(&pk_vec) {
                    continue;
                }
                if gc_vcs::verify_commit_attestation(&at, &signing_h, &class.allowed_public_keys)
                    .is_ok()
                {
                    seen_pks.insert(pk_vec);
                    valid = valid.saturating_add(1);
                }
            }
            if valid < class.min_signatures {
                return Err(mk_error(
                    error_tok,
                    "core/refs/insufficient-signatures",
                    format!(
                        "need {} valid signatures, got {valid}",
                        class.min_signatures
                    ),
                    Some(op),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn payload_refs_name(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":name".to_string()))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err(EffectsError::Log(
            "core/refs payload missing :name".to_string(),
        )),
    }
}

pub(crate) fn payload_refs_prefix(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    Ok(
        match m.get(&TermOrdKey(Term::Symbol(":prefix".to_string()))) {
            Some(Term::Str(s)) => Some(s.clone()),
            Some(Term::Nil) | None => None,
            _ => {
                return Err(EffectsError::Log(
                    "core/refs payload :prefix must be string or nil".to_string(),
                ));
            }
        },
    )
}

pub(crate) fn payload_refs_hash(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    Ok(
        match m.get(&TermOrdKey(Term::Symbol(":hash".to_string()))) {
            Some(Term::Str(s)) => Some(s.clone()),
            Some(Term::Nil) | None => None,
            _ => {
                return Err(EffectsError::Log(
                    "core/refs payload :hash must be string or nil".to_string(),
                ));
            }
        },
    )
}

pub(crate) fn payload_refs_expected_old(
    payload: &Term,
) -> Result<Option<Option<String>>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":expected-old".to_string()))) {
        None => Ok(None),
        Some(Term::Nil) => Ok(Some(None)),
        Some(Term::Str(s)) => Ok(Some(Some(s.clone()))),
        _ => Err(EffectsError::Log(
            "core/refs payload :expected-old must be string, nil, or absent".to_string(),
        )),
    }
}

pub(crate) fn payload_refs_policy_hash(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":policy".to_string()))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err(EffectsError::Log(
            "core/refs payload missing :policy".to_string(),
        )),
    }
}

fn mk_error(error_tok: SealId, code: &str, msg: String, op: Option<&str>) -> Value {
    let mut mm = BTreeMap::new();
    mm.insert(
        TermOrdKey(Term::symbol(":error/code")),
        Term::Str(code.to_string()),
    );
    mm.insert(TermOrdKey(Term::symbol(":error/message")), Term::Str(msg));
    mm.insert(
        TermOrdKey(Term::symbol(":error/op")),
        op.map(Term::symbol).unwrap_or(Term::Nil),
    );
    mm.insert(TermOrdKey(Term::symbol(":error/context")), Term::Nil);
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(mm))),
    }
}
