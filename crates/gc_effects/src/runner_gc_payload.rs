use gc_coreform::{Term, TermOrdKey, print_term};
use num_traits::ToPrimitive;

use crate::EffectsError;

pub(crate) fn payload_gc_lock(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":lock"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn payload_gc_pins(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":pins"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn payload_gc_depth(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":depth"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

pub(crate) fn payload_gc_include_lock(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":include-lock"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

pub(crate) fn payload_gc_include_refs(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":include-refs"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

pub(crate) fn payload_gc_quarantine(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":quarantine"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

pub(crate) fn payload_gc_quarantine_dir(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":quarantine-dir"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn payload_gc_ttl_days(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":ttl-days"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

pub(crate) fn payload_gc_target(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(":target")))
        .ok_or_else(|| EffectsError::BadPayload("missing :target".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        Term::Symbol(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":target must be string/symbol, got {}",
            print_term(v)
        ))),
    }
}
