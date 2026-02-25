use gc_coreform::{Term, TermOrdKey, print_term};
use num_traits::ToPrimitive;

pub(crate) fn payload_pkg_lock(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":lock"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        None => Ok("genesis.lock".to_string()),
        Some(other) => Err(format!(":lock must be string, got {}", print_term(other))),
    }
}

pub(crate) fn payload_pkg_workspace(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":workspace"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :workspace string".to_string()),
    }
}

pub(crate) fn payload_pkg_policy(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":policy"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn payload_pkg_registry_default(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":registry-default"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn payload_pkg_name(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":name"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :name string".to_string()),
    }
}

pub(crate) fn payload_pkg_selector(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":selector"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :selector string".to_string()),
    }
}

pub(crate) fn payload_pkg_registry(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":registry"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn payload_pkg_update_policy(
    payload: &Term,
) -> Result<Option<gc_pkg::UpdatePolicy>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":update-policy"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => match s.as_str() {
            "auto" => Ok(Some(gc_pkg::UpdatePolicy::Auto)),
            "manual" => Ok(Some(gc_pkg::UpdatePolicy::Manual)),
            other => Err(format!(
                ":update-policy must be 'manual' or 'auto', got {other}"
            )),
        },
        Some(other) => Err(format!(
            ":update-policy must be string or nil, got {}",
            print_term(other)
        )),
    }
}

pub(crate) fn payload_pkg_strategy(
    payload: &Term,
) -> Result<Option<gc_pkg::ResolutionStrategy>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":strategy"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Symbol(s)) => {
            let raw = s.trim_start_matches(':');
            raw.parse::<gc_pkg::ResolutionStrategy>()
                .map(Some)
                .map_err(|_| format!(":strategy must be :pinned|:track-ref|:tag-policy, got {s}"))
        }
        Some(Term::Str(s)) => s
            .parse::<gc_pkg::ResolutionStrategy>()
            .map(Some)
            .map_err(|_| format!(":strategy must be pinned|track-ref|tag-policy, got {s}")),
        Some(other) => Err(format!(
            ":strategy must be symbol/string or nil, got {}",
            print_term(other)
        )),
    }
}

pub(crate) fn payload_pkg_tag_policy(payload: &Term) -> Result<Option<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":tag-policy"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!(
            ":tag-policy must be string or nil, got {}",
            print_term(other)
        )),
    }
}

pub(crate) fn payload_pkg_bool(payload: &Term, key: &str) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

pub(crate) fn payload_pkg_only(payload: &Term) -> Result<Option<Vec<String>>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":only"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Vector(xs)) => {
            let mut out = Vec::with_capacity(xs.len());
            for (idx, x) in xs.iter().enumerate() {
                match x {
                    Term::Str(s) => out.push(s.clone()),
                    other => {
                        return Err(format!(
                            ":only entries must be strings (entry {idx} got {})",
                            print_term(other)
                        ));
                    }
                }
            }
            Ok(Some(out))
        }
        Some(other) => Err(format!(
            ":only must be vector<string> or nil, got {}",
            print_term(other)
        )),
    }
}

pub(crate) fn payload_pkg_publish_remote(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":remote"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :remote string".to_string()),
    }
}

pub(crate) fn payload_pkg_publish_ref(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":ref"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :ref string".to_string()),
    }
}

pub(crate) fn payload_pkg_publish_policy(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":policy"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :policy string".to_string()),
    }
}

pub(crate) fn payload_pkg_publish_expected_old(payload: &Term) -> Result<Option<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":expected-old"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!(
            ":expected-old must be string or nil, got {}",
            print_term(other)
        )),
    }
}

pub(crate) fn payload_pkg_publish_depth(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":depth"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

pub(crate) fn payload_pkg_publish_commit(payload: &Term) -> Result<Option<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":commit"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!(
            ":commit must be string or nil, got {}",
            print_term(other)
        )),
    }
}

fn payload_pkg_bridge_required_string(payload: &Term, key: &str) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Err(format!("{key} must not be empty"))
            } else {
                Ok(trimmed.to_string())
            }
        }
        _ => Err(format!("missing {key} string")),
    }
}

fn is_hex_len(s: &str, len: usize) -> bool {
    s.len() == len && s.as_bytes().iter().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn payload_pkg_bridge_ecosystem(payload: &Term) -> Result<String, String> {
    payload_pkg_bridge_required_string(payload, ":ecosystem")
}

pub(crate) fn payload_pkg_bridge_version(payload: &Term) -> Result<String, String> {
    payload_pkg_bridge_required_string(payload, ":version")
}

pub(crate) fn payload_pkg_bridge_source(payload: &Term) -> Result<String, String> {
    payload_pkg_bridge_required_string(payload, ":source")
}

pub(crate) fn payload_pkg_bridge_source_hash(payload: &Term) -> Result<String, String> {
    let h = payload_pkg_bridge_required_string(payload, ":source-hash")?;
    if !is_hex_len(&h, 64) {
        return Err(":source-hash must be 64-hex".to_string());
    }
    Ok(h.to_ascii_lowercase())
}

pub(crate) fn payload_pkg_bridge_key_id(payload: &Term) -> Result<String, String> {
    payload_pkg_bridge_required_string(payload, ":key-id")
}

pub(crate) fn payload_pkg_bridge_public_key(payload: &Term) -> Result<String, String> {
    let pk = payload_pkg_bridge_required_string(payload, ":public-key")?;
    if !is_hex_len(&pk, 64) {
        return Err(":public-key must be 64-hex".to_string());
    }
    Ok(pk.to_ascii_lowercase())
}

pub(crate) fn payload_pkg_bridge_lock(payload: &Term) -> Result<Option<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":lock"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Err(":lock must not be empty when provided".to_string())
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Some(other) => Err(format!(
            ":lock must be string or nil, got {}",
            print_term(other)
        )),
    }
}

pub(crate) fn payload_pkg_bridge_dep_name(payload: &Term) -> Result<Option<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":dep-name"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Err(":dep-name must not be empty when provided".to_string())
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Some(other) => Err(format!(
            ":dep-name must be string or nil, got {}",
            print_term(other)
        )),
    }
}
