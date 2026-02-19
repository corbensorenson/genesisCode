use gc_coreform::{Term, TermOrdKey, print_term};
use num_traits::ToPrimitive;

use crate::EffectsError;

pub(crate) fn payload_vcs_root(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(":root")))
        .ok_or_else(|| EffectsError::BadPayload("missing :root".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        Term::Symbol(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":root must be string/symbol, got {}",
            print_term(v)
        ))),
    }
}

pub(crate) fn payload_vcs_max(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":max"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

pub(crate) fn payload_vcs_out(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::symbol(":out"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(EffectsError::BadPayload(format!(
            ":out must be string or nil, got {}",
            print_term(other)
        ))),
    }
}

pub(crate) fn payload_vcs_store(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":store"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

pub(crate) fn payload_vcs_patch(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(":patch")))
        .ok_or_else(|| EffectsError::BadPayload("missing :patch".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        Term::Symbol(s) => Ok(s.clone()),
        other => Err(EffectsError::BadPayload(format!(
            ":patch must be string/symbol, got {}",
            print_term(other)
        ))),
    }
}

pub(crate) fn payload_vcs_hash(payload: &Term, key: &str) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| EffectsError::BadPayload(format!("missing {key}")))?;
    match v {
        Term::Str(s) => {
            gc_vcs::validate_hex_hash(s)
                .map_err(|e| EffectsError::BadPayload(format!("{key}: {e}")))?;
            Ok(s.clone())
        }
        other => Err(EffectsError::BadPayload(format!(
            "{key} must be hex string, got {}",
            print_term(other)
        ))),
    }
}

pub(crate) fn payload_vcs_opt_hash(
    payload: &Term,
    key: &str,
) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => {
            gc_vcs::validate_hex_hash(s)
                .map_err(|e| EffectsError::BadPayload(format!("{key}: {e}")))?;
            Ok(Some(s.clone()))
        }
        Some(other) => Err(EffectsError::BadPayload(format!(
            "{key} must be hex string or nil, got {}",
            print_term(other)
        ))),
    }
}

pub(crate) fn payload_vcs_sym(payload: &Term, key: &str) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| EffectsError::BadPayload(format!("missing {key}")))?;
    match v {
        Term::Symbol(s) => Ok(s.clone()),
        Term::Str(s) => Ok(s.clone()),
        other => Err(EffectsError::BadPayload(format!(
            "{key} must be symbol/string, got {}",
            print_term(other)
        ))),
    }
}

pub(crate) fn payload_vcs_opt_sym_or_str(
    payload: &Term,
    key: &str,
) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Symbol(s)) => Ok(Some(s.clone())),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(EffectsError::BadPayload(format!(
            "{key} must be symbol/string or nil, got {}",
            print_term(other)
        ))),
    }
}
