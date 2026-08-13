use super::*;

pub(super) fn input(value: Option<&toml::Value>) -> Term {
    match value {
        None => Term::Nil,
        Some(value) => match value.as_array() {
            Some(values) => Term::Vector(
                values
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(|value| Term::Str(value.to_string()))
                            .unwrap_or_else(|| Term::symbol(":invalid-entry"))
                    })
                    .collect(),
            ),
            None => Term::symbol(":invalid-type"),
        },
    }
}

pub(super) fn decode(
    term: &Term,
    allowed: bool,
) -> Result<AuthorizedProcessPrograms, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(AuthorizedProcessPrograms::Absent)
        } else {
            Err(authority_error(
                "denied result :process-program-policy must be nil",
            ))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error(
            "admitted result :process-program-policy must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [":programs", ":status"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(
            "result :process-program-policy field set mismatch",
        ));
    }
    let status = match map.get(&TermOrdKey(Term::symbol(":status"))) {
        Some(Term::Symbol(status)) => status.as_str(),
        _ => {
            return Err(authority_error(
                "result :process-program-policy :status must be a symbol",
            ));
        }
    };
    let programs = map
        .get(&TermOrdKey(Term::symbol(":programs")))
        .ok_or_else(|| authority_error("result :process-program-policy is missing :programs"))?;
    match (status, programs) {
        (":absent", Term::Nil) => Ok(AuthorizedProcessPrograms::Absent),
        (":invalid-type", Term::Nil) => Ok(AuthorizedProcessPrograms::InvalidType),
        (":invalid-entry", Term::Nil) => Ok(AuthorizedProcessPrograms::InvalidEntry),
        (":empty", Term::Nil) => Ok(AuthorizedProcessPrograms::Empty),
        (":valid", Term::Vector(values)) if !values.is_empty() => values
            .iter()
            .map(|value| match value {
                Term::Str(value) if !value.is_empty() && value.trim() == value => {
                    Ok(value.clone())
                }
                _ => Err(authority_error(
                    "result :process-program-policy valid programs must be nonempty canonical strings",
                )),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(AuthorizedProcessPrograms::Valid),
        _ => Err(authority_error(
            "result :process-program-policy status contradicts its programs",
        )),
    }
}
