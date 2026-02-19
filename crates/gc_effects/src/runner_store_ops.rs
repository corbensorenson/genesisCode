use gc_coreform::{Term, TermOrdKey};

use crate::EffectsError;
use crate::store::ArtifactStore;

pub(crate) fn payload_store_hash(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/store payload must be a map".to_string(),
        ));
    };
    let Some(Term::Str(h)) = m.get(&TermOrdKey(Term::Symbol(":hash".to_string()))) else {
        return Err(EffectsError::Log(
            "core/store payload missing :hash".to_string(),
        ));
    };
    Ok(h.clone())
}

pub(crate) fn payload_store_optional_hash(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/store payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":hash".to_string()))) {
        Some(Term::Str(h)) => Ok(Some(h.clone())),
        Some(Term::Nil) | None => Ok(None),
        _ => Err(EffectsError::Log(
            "core/store payload :hash must be string or nil".to_string(),
        )),
    }
}

pub(crate) fn payload_store_artifact(payload: &Term) -> Result<Term, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/store payload must be a map".to_string(),
        ));
    };
    let Some(t) = m.get(&TermOrdKey(Term::Symbol(":artifact".to_string()))) else {
        return Err(EffectsError::Log(
            "core/store payload missing :artifact".to_string(),
        ));
    };
    Ok(t.clone())
}

pub(crate) fn store_get_term(store: &ArtifactStore, hex: &str) -> Result<Term, EffectsError> {
    let bytes = store.get_bytes(hex)?;
    let s = String::from_utf8(bytes)
        .map_err(|_| EffectsError::Log("artifact bytes are not utf-8 term".to_string()))?;
    gc_coreform::parse_term(&s).map_err(|e| EffectsError::Log(format!("bad artifact term: {e}")))
}

pub(crate) fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn store_scan_hashes(store: &ArtifactStore) -> Result<Vec<String>, EffectsError> {
    let mut out = Vec::new();
    for ent in std::fs::read_dir(store.root_dir())? {
        let ent = ent?;
        if !ent.file_type()?.is_file() {
            continue;
        }
        let Some(name) = ent.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        if name.starts_with(".tmp-") || !is_hex64(&name) {
            continue;
        }
        out.push(name);
    }
    out.sort();
    Ok(out)
}
