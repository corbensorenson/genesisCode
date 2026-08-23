use super::*;

pub(super) fn cap_refs_get(
    payload: &Term,
    refs: Option<&RefsDb>,
    authority: Option<&mut RefsAuthority>,
) -> Result<Value, EffectsError> {
    let refs =
        refs.ok_or_else(|| EffectsError::Log("missing refs db for core/refs::get".to_string()))?;
    let name = payload_refs_name(payload)?;
    let authority = authority.ok_or_else(|| {
        EffectsError::Log(
            "core/refs::get requires the artifact-loaded GenesisCode refs authority".to_string(),
        )
    })?;
    let h = authority.get(refs, &name)?;
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":name".to_string())),
        Term::Str(name),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":hash".to_string())),
        h.map(Term::Str).unwrap_or(Term::Nil),
    );
    Ok(Value::data(Term::Map(m)))
}

pub(super) fn cap_refs_list(
    payload: &Term,
    refs: Option<&RefsDb>,
    authority: Option<&mut RefsAuthority>,
) -> Result<Value, EffectsError> {
    let refs =
        refs.ok_or_else(|| EffectsError::Log("missing refs db for core/refs::list".to_string()))?;
    let prefix = payload_refs_prefix(payload)?;
    let authority = authority.ok_or_else(|| {
        EffectsError::Log(
            "core/refs::list requires the artifact-loaded GenesisCode refs authority".to_string(),
        )
    })?;
    let xs = authority.list(refs, prefix.as_deref())?;
    let mut out = Vec::new();
    for e in xs {
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::Symbol(":name".to_string())),
            Term::Str(e.name),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":hash".to_string())),
            e.hash.map(Term::Str).unwrap_or(Term::Nil),
        );
        out.push(Term::Map(m));
    }
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":refs".to_string())),
        Term::Vector(out),
    );
    Ok(Value::data(Term::Map(m)))
}

pub(super) fn cap_refs_set(
    op: &str,
    payload: &Term,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    authority: Option<&mut RefsAuthority>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/refs::set".to_string())
    })?;
    let refs =
        refs.ok_or_else(|| EffectsError::Log("missing refs db for core/refs::set".to_string()))?;

    let name = payload_refs_name(payload)?;
    let new_hash = payload_refs_hash(payload)?;
    let expected_old = payload_refs_expected_old(payload)?;
    let policy_h = payload_refs_policy_hash(payload)?;
    let authority = authority.ok_or_else(|| {
        EffectsError::Log(
            "core/refs::set requires the artifact-loaded GenesisCode refs authority".to_string(),
        )
    })?;
    if let Err(value) = local_refs_validate_policy_gate(
        authority,
        store,
        &name,
        new_hash.as_deref(),
        &policy_h,
        error_tok,
        op,
    ) {
        return Ok(value);
    }
    let result = authority.set(
        refs,
        &name,
        new_hash.as_deref(),
        expected_old.as_ref().map(|value| value.as_deref()),
    )?;

    match result {
        SetResult::Updated => {
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":ok".to_string())),
                Term::Bool(true),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":name".to_string())),
                Term::Str(name),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":hash".to_string())),
                new_hash.map(Term::Str).unwrap_or(Term::Nil),
            );
            Ok(Value::data(Term::Map(m)))
        }
        SetResult::Conflict { current } => Ok(mk_error_with_ctx(
            error_tok,
            "core/refs/conflict",
            "ref update conflict".to_string(),
            Some(op),
            Term::Map(
                [(
                    TermOrdKey(Term::Symbol(":refs/current".to_string())),
                    current.map(Term::Str).unwrap_or(Term::Nil),
                )]
                .into_iter()
                .collect(),
            ),
        )),
    }
}

pub(super) fn cap_refs_delete(
    op: &str,
    payload: &Term,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    authority: Option<&mut RefsAuthority>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let store = store.ok_or_else(|| {
        EffectsError::Log("missing artifact store for core/refs::delete".to_string())
    })?;
    let refs =
        refs.ok_or_else(|| EffectsError::Log("missing refs db for core/refs::delete".to_string()))?;
    let name = payload_refs_name(payload)?;
    let expected_old = payload_refs_expected_old(payload)?;
    let policy_h = payload_refs_policy_hash(payload)?;
    let authority = authority.ok_or_else(|| {
        EffectsError::Log(
            "core/refs::delete requires the artifact-loaded GenesisCode refs authority".to_string(),
        )
    })?;
    if let Err(value) =
        local_refs_validate_policy_gate(authority, store, &name, None, &policy_h, error_tok, op)
    {
        return Ok(value);
    }
    let result = authority.set(
        refs,
        &name,
        None,
        expected_old.as_ref().map(|value| value.as_deref()),
    )?;

    match result {
        SetResult::Updated => {
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":ok".to_string())),
                Term::Bool(true),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":name".to_string())),
                Term::Str(name),
            );
            Ok(Value::data(Term::Map(m)))
        }
        SetResult::Conflict { current } => Ok(mk_error_with_ctx(
            error_tok,
            "core/refs/conflict",
            "ref delete conflict".to_string(),
            Some(op),
            Term::Map(
                [(
                    TermOrdKey(Term::Symbol(":refs/current".to_string())),
                    current.map(Term::Str).unwrap_or(Term::Nil),
                )]
                .into_iter()
                .collect(),
            ),
        )),
    }
}
