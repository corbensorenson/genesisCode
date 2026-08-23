use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};

use crate::EffectsError;
use crate::refs_authority::{RefsAuthority, RefsPolicyDecision};
use crate::store::ArtifactStore;

pub(crate) fn local_refs_validate_policy_gate(
    authority: &mut RefsAuthority,
    store: &ArtifactStore,
    name: &str,
    new_hash: Option<&str>,
    policy_h: &str,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    match authority.validate_policy_gate(store, name, new_hash, policy_h) {
        Ok(RefsPolicyDecision::Accept) => Ok(()),
        Ok(RefsPolicyDecision::Error { code, message }) => {
            Err(mk_error(error_tok, &code, message, Some(op)))
        }
        Err(error) => Err(mk_error(
            error_tok,
            "core/refs/bad-authority-request",
            error.to_string(),
            Some(op),
        )),
    }
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
        payload: Box::new(Value::data(Term::Map(mm))),
    }
}
