use std::collections::BTreeSet;

use gc_coreform::{Term, TermOrdKey, print_term};
use num_traits::ToPrimitive;

#[cfg(not(target_os = "wasi"))]
pub(crate) fn payload_sync_remote(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":remote"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :remote string".to_string()),
    }
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn payload_sync_refs(payload: &Term) -> Result<Vec<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(":refs must be vector, got {}", print_term(t)));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            other => {
                return Err(format!(
                    ":refs entries must be strings, got {}",
                    print_term(other)
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn payload_sync_roots(payload: &Term) -> Result<Vec<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":roots"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(":roots must be vector, got {}", print_term(t)));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            other => {
                return Err(format!(
                    ":roots entries must be strings, got {}",
                    print_term(other)
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn payload_sync_depth(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":depth"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn payload_sync_force(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":force"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone)]
pub(crate) struct SyncSetRef {
    pub(crate) name: String,
    pub(crate) hash: String,
    pub(crate) policy: String,
    pub(crate) expected_old: Option<String>,
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn payload_sync_set_refs(payload: &Term) -> Result<Vec<SyncSetRef>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":set-refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(":set-refs must be vector, got {}", print_term(t)));
    };
    let mut out = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for x in xs {
        let Term::Map(mm) = x else {
            return Err(format!(
                ":set-refs entries must be maps, got {}",
                print_term(x)
            ));
        };
        let name = match mm.get(&TermOrdKey(Term::symbol(":name"))) {
            Some(Term::Str(s)) if !s.trim().is_empty() => s.clone(),
            _ => return Err("set-ref missing :name string".to_string()),
        };
        if !seen.insert(name.clone()) {
            return Err(format!("duplicate set-ref target: {name}"));
        }
        let hash = match mm.get(&TermOrdKey(Term::symbol(":hash"))) {
            Some(Term::Str(s)) if gc_vcs::validate_hex_hash(s).is_ok() => s.to_ascii_lowercase(),
            Some(Term::Str(_)) => return Err("set-ref :hash must be 64-hex".to_string()),
            _ => return Err("set-ref missing :hash string".to_string()),
        };
        let policy = match mm.get(&TermOrdKey(Term::symbol(":policy"))) {
            Some(Term::Str(s)) if gc_vcs::validate_hex_hash(s).is_ok() => s.to_ascii_lowercase(),
            Some(Term::Str(_)) => return Err("set-ref :policy must be 64-hex".to_string()),
            _ => return Err("set-ref missing :policy string".to_string()),
        };
        let expected_old = match mm.get(&TermOrdKey(Term::symbol(":expected-old"))) {
            None | Some(Term::Nil) => None,
            Some(Term::Str(s)) if s == "nil" => Some("nil".to_string()),
            Some(Term::Str(s)) if gc_vcs::validate_hex_hash(s).is_ok() => {
                Some(s.to_ascii_lowercase())
            }
            Some(Term::Str(_)) => {
                return Err("set-ref :expected-old must be 64-hex or nil".to_string());
            }
            Some(other) => {
                return Err(format!(
                    "set-ref :expected-old must be string or nil, got {}",
                    print_term(other)
                ));
            }
        };
        out.push(SyncSetRef {
            name,
            hash,
            policy,
            expected_old,
        });
    }
    Ok(out)
}
