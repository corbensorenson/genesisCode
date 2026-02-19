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
