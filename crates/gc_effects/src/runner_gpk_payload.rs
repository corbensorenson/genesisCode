use std::collections::BTreeSet;

use gc_coreform::{Term, TermOrdKey, print_term};
use num_traits::ToPrimitive;

use crate::EffectsError;

pub(crate) fn payload_gpk_root(payload: &Term) -> Result<String, EffectsError> {
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

pub(crate) fn payload_gpk_out(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(":out")))
        .ok_or_else(|| EffectsError::BadPayload("missing :out".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":out must be string, got {}",
            print_term(v)
        ))),
    }
}

pub(crate) fn payload_gpk_in(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(":in")))
        .ok_or_else(|| EffectsError::BadPayload("missing :in".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":in must be string, got {}",
            print_term(v)
        ))),
    }
}

pub(crate) fn payload_gpk_mode(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::symbol(":mode"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Symbol(s)) => Ok(Some(s.clone())),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(EffectsError::BadPayload(format!(
            ":mode must be symbol/string or nil, got {}",
            print_term(other)
        ))),
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpkMode {
    Shallow,
    Full,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpkIncludeEvidence {
    Required,
    All,
    None,
}

impl GpkIncludeEvidence {
    pub(crate) fn from_token(s: &str) -> Option<Self> {
        match s {
            "required" | ":required" => Some(Self::Required),
            "all" | ":all" => Some(Self::All),
            "none" | ":none" => Some(Self::None),
            _ => None,
        }
    }

    pub(crate) fn to_symbol(self) -> &'static str {
        match self {
            Self::Required => ":required",
            Self::All => ":all",
            Self::None => ":none",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpkIncludeDeps {
    None,
    Locked,
    All,
}

impl GpkIncludeDeps {
    pub(crate) fn from_token(s: &str) -> Option<Self> {
        match s {
            "none" | ":none" => Some(Self::None),
            "locked" | ":locked" => Some(Self::Locked),
            "all" | ":all" => Some(Self::All),
            _ => None,
        }
    }

    pub(crate) fn to_symbol(self) -> &'static str {
        match self {
            Self::None => ":none",
            Self::Locked => ":locked",
            Self::All => ":all",
        }
    }
}

pub(crate) fn payload_gpk_depth(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":depth"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

pub(crate) fn payload_gpk_refs(payload: &Term) -> Result<Vec<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(EffectsError::BadPayload(format!(
            ":refs must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            Term::Symbol(s) => out.push(s.clone()),
            other => {
                return Err(EffectsError::BadPayload(format!(
                    ":refs entries must be strings/symbols, got {}",
                    print_term(other)
                )));
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub(crate) struct GpkSetRef {
    pub(crate) name: String,
    pub(crate) hash: Option<String>,
    pub(crate) policy: String,
    pub(crate) expected_old: Option<Option<String>>,
}

pub(crate) fn payload_gpk_set_refs(payload: &Term) -> Result<Vec<GpkSetRef>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":set-refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(EffectsError::BadPayload(format!(
            ":set-refs must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::with_capacity(xs.len());
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for x in xs {
        let Term::Map(mm) = x else {
            return Err(EffectsError::BadPayload(format!(
                ":set-refs entries must be maps, got {}",
                print_term(x)
            )));
        };
        let name = match mm.get(&TermOrdKey(Term::symbol(":name"))) {
            Some(Term::Str(s)) if !s.trim().is_empty() => s.clone(),
            _ => {
                return Err(EffectsError::BadPayload(
                    "set-ref missing :name string".to_string(),
                ));
            }
        };
        if !seen.insert(name.clone()) {
            return Err(EffectsError::BadPayload(format!(
                "duplicate set-ref target: {name}"
            )));
        }
        let hash = match mm.get(&TermOrdKey(Term::symbol(":hash"))) {
            None | Some(Term::Nil) => None,
            Some(Term::Str(s)) if s == "nil" => None,
            Some(Term::Str(s)) => {
                if gc_vcs::validate_hex_hash(s).is_err() {
                    return Err(EffectsError::BadPayload(
                        "set-ref :hash must be 64-hex or nil".to_string(),
                    ));
                }
                Some(s.to_ascii_lowercase())
            }
            Some(other) => {
                return Err(EffectsError::BadPayload(format!(
                    "set-ref :hash must be string or nil, got {}",
                    print_term(other)
                )));
            }
        };
        let policy = match mm.get(&TermOrdKey(Term::symbol(":policy"))) {
            Some(Term::Str(s)) if !s.trim().is_empty() => {
                if gc_vcs::validate_hex_hash(s).is_err() {
                    return Err(EffectsError::BadPayload(
                        "set-ref :policy must be 64-hex".to_string(),
                    ));
                }
                s.to_ascii_lowercase()
            }
            _ => {
                return Err(EffectsError::BadPayload(
                    "set-ref missing :policy string".to_string(),
                ));
            }
        };
        let expected_old = match mm.get(&TermOrdKey(Term::symbol(":expected-old"))) {
            None => None,
            Some(Term::Nil) => Some(None),
            Some(Term::Str(s)) if s == "nil" => Some(None),
            Some(Term::Str(s)) => {
                if gc_vcs::validate_hex_hash(s).is_err() {
                    return Err(EffectsError::BadPayload(
                        "set-ref :expected-old must be 64-hex, nil, or absent".to_string(),
                    ));
                }
                Some(Some(s.to_ascii_lowercase()))
            }
            Some(other) => {
                return Err(EffectsError::BadPayload(format!(
                    "set-ref :expected-old must be string, nil, or absent, got {}",
                    print_term(other)
                )));
            }
        };
        out.push(GpkSetRef {
            name,
            hash,
            policy,
            expected_old,
        });
    }
    Ok(out)
}

pub(crate) fn payload_gpk_include_evidence(
    payload: &Term,
) -> Result<GpkIncludeEvidence, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m.get(&TermOrdKey(Term::symbol(":include-evidence")));
    let Some(v) = v else {
        return Ok(GpkIncludeEvidence::Required);
    };
    let token = match v {
        Term::Str(s) | Term::Symbol(s) => s.as_str(),
        other => {
            return Err(EffectsError::BadPayload(format!(
                ":include-evidence must be symbol/string, got {}",
                print_term(other)
            )));
        }
    };
    GpkIncludeEvidence::from_token(token).ok_or_else(|| {
        EffectsError::BadPayload(format!(
            ":include-evidence must be one of required|all|none, got {token}"
        ))
    })
}

pub(crate) fn payload_gpk_include_deps(payload: &Term) -> Result<GpkIncludeDeps, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m.get(&TermOrdKey(Term::symbol(":include-deps")));
    let Some(v) = v else {
        return Ok(GpkIncludeDeps::Locked);
    };
    let token = match v {
        Term::Str(s) | Term::Symbol(s) => s.as_str(),
        other => {
            return Err(EffectsError::BadPayload(format!(
                ":include-deps must be symbol/string, got {}",
                print_term(other)
            )));
        }
    };
    GpkIncludeDeps::from_token(token).ok_or_else(|| {
        EffectsError::BadPayload(format!(
            ":include-deps must be one of none|locked|all, got {token}"
        ))
    })
}

pub(crate) fn payload_data(payload: &Term) -> Result<Vec<u8>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(":data")))
        .ok_or_else(|| EffectsError::BadPayload("missing :data".to_string()))?;
    match v {
        Term::Bytes(b) => Ok(b.to_vec()),
        Term::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(EffectsError::BadPayload(format!(
            ":data must be bytes or string, got {}",
            print_term(v)
        ))),
    }
}
