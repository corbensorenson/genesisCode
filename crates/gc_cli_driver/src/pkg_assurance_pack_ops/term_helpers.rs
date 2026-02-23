use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, print_term};
use gc_vcs::validate_hex_hash;

pub(super) fn extract_release_binding(term: &Term, key: &str) -> Result<Option<String>, String> {
    let root = as_map(term, "artifact")?;
    let release_term = root
        .get(&TermOrdKey(Term::symbol(":release")))
        .ok_or_else(|| "artifact missing :release".to_string())?;
    let release = as_map(release_term, "artifact/:release")?;
    match release.get(&TermOrdKey(Term::symbol(key))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => {
            if s.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(s.clone()))
            }
        }
        Some(other) => Err(format!(
            "artifact/:release {key} must be string|nil, got {}",
            print_term(other)
        )),
    }
}

pub(super) fn extract_vector_field<'a>(
    term: &'a Term,
    key: &str,
    what: &str,
) -> Result<&'a [Term], String> {
    let root = as_map(term, what)?;
    match root.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Vector(v)) => Ok(v.as_slice()),
        Some(other) => Err(format!(
            "{what} {key} must be vector, got {}",
            print_term(other)
        )),
        None => Err(format!("{what} missing {key}")),
    }
}

pub(super) fn as_map<'a>(
    term: &'a Term,
    what: &str,
) -> Result<&'a BTreeMap<TermOrdKey, Term>, String> {
    match term {
        Term::Map(m) => Ok(m),
        _ => Err(format!("{what} must be map, got {}", print_term(term))),
    }
}

pub(super) fn required_symbol_or_string(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    match t {
        Term::Symbol(s) | Term::Str(s) => Ok(s.clone()),
        _ => Err(format!("{what} {key} must be symbol|string")),
    }
}

pub(super) fn required_string(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    match t {
        Term::Str(s) => {
            if s.trim().is_empty() {
                Err(format!("{what} {key} cannot be empty"))
            } else {
                Ok(s.clone())
            }
        }
        _ => Err(format!("{what} {key} must be string")),
    }
}

pub(super) fn required_hex64(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    let value = required_string(m, key, what)?;
    validate_hex_hash(&value).map_err(|e| format!("{what} {key} must be hex64: {e}"))?;
    Ok(value)
}

pub(super) fn required_symbol_vector(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<Vec<String>, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    let Term::Vector(values) = t else {
        return Err(format!("{what} {key} must be vector"));
    };
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        match value {
            Term::Symbol(s) | Term::Str(s) => out.push(normalize_symbol_like(s)),
            _ => {
                return Err(format!(
                    "{what} {key} entries must be symbols|strings, got {}",
                    print_term(value)
                ));
            }
        }
    }
    Ok(out)
}

pub(super) fn required_bool(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<bool, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    match t {
        Term::Bool(v) => Ok(*v),
        _ => Err(format!("{what} {key} must be bool")),
    }
}

pub(super) fn normalize_symbol_like(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with(':') {
        trimmed.to_string()
    } else {
        format!(":{trimmed}")
    }
}

pub(super) fn is_hex64(s: &str) -> bool {
    if s.len() != 64 {
        return false;
    }
    validate_hex_hash(s).is_ok()
}

pub(super) fn coverage_rank(profile: &str) -> u8 {
    match normalize_symbol_like(profile).as_str() {
        ":symbol" => 1,
        ":decision" => 2,
        ":mcdc" => 3,
        _ => 0,
    }
}
