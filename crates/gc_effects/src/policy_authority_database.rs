use super::*;
use crate::policy::AuthorizedStringList;

pub(super) fn string_list_input(value: Option<&toml::Value>) -> Term {
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

pub(super) fn input(table: &toml::value::Table) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":max-result-bytes")),
                max_bytes_input(table.get("max_result_bytes")),
            ),
            (
                TermOrdKey(Term::symbol(":max-row-count")),
                max_bytes_input(table.get("max_row_count")),
            ),
            (
                TermOrdKey(Term::symbol(":max-value-bytes")),
                max_bytes_input(table.get("max_value_bytes")),
            ),
            (
                TermOrdKey(Term::symbol(":query-classes")),
                string_list_input(table.get("allow_query_classes")),
            ),
            (
                TermOrdKey(Term::symbol(":target-allow")),
                string_list_input(table.get("db_target_allow")),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn legacy_string_list(value: Option<&toml::Value>) -> AuthorizedStringList {
    let Some(value) = value else {
        return AuthorizedStringList::Absent;
    };
    let Some(values) = value.as_array() else {
        return AuthorizedStringList::InvalidType;
    };
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let Some(value) = value.as_str() else {
            return AuthorizedStringList::InvalidEntry;
        };
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        AuthorizedStringList::Empty
    } else {
        AuthorizedStringList::Valid(out)
    }
}

pub(super) fn legacy_positive(value: Option<&toml::Value>) -> AuthorizedMaxBytes {
    let Some(value) = value else {
        return AuthorizedMaxBytes::Absent;
    };
    let Some(raw) = value.as_integer() else {
        return AuthorizedMaxBytes::InvalidType;
    };
    if raw <= 0 {
        return AuthorizedMaxBytes::NonPositive;
    }
    usize::try_from(raw)
        .map(AuthorizedMaxBytes::Valid)
        .unwrap_or(AuthorizedMaxBytes::PlatformOverflow)
}

pub(super) fn legacy(policy: Option<&OpPolicy>) -> AuthorizedDatabasePolicy {
    let extra = policy.map(|policy| &policy.extra);
    let get = |key| extra.and_then(|extra| extra.get(key));
    AuthorizedDatabasePolicy {
        target_allow: legacy_string_list(get("db_target_allow")),
        query_classes: legacy_string_list(get("allow_query_classes")),
        max_result_bytes: legacy_positive(get("max_result_bytes")),
        max_row_count: legacy_positive(get("max_row_count")),
        max_value_bytes: legacy_positive(get("max_value_bytes")),
    }
}

pub(super) fn decode_string_list(
    term: &Term,
    field: &str,
) -> Result<AuthorizedStringList, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error(format!(
            "result {field} must be a data map"
        )));
    };
    let expected: BTreeSet<_> = [":status", ":values"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(format!(
            "result {field} field set mismatch"
        )));
    }
    let status = match map.get(&TermOrdKey(Term::symbol(":status"))) {
        Some(Term::Symbol(status)) => status.as_str(),
        _ => {
            return Err(authority_error(format!(
                "result {field} :status must be a symbol"
            )));
        }
    };
    let values = map
        .get(&TermOrdKey(Term::symbol(":values")))
        .ok_or_else(|| authority_error(format!("result {field} is missing :values")))?;
    match (status, values) {
        (":absent", Term::Nil) => Ok(AuthorizedStringList::Absent),
        (":invalid-type", Term::Nil) => Ok(AuthorizedStringList::InvalidType),
        (":invalid-entry", Term::Nil) => Ok(AuthorizedStringList::InvalidEntry),
        (":empty", Term::Nil) => Ok(AuthorizedStringList::Empty),
        (":valid", Term::Vector(values)) if !values.is_empty() => values
            .iter()
            .map(|value| match value {
                Term::Str(value) if !value.is_empty() && value.trim() == value => Ok(value.clone()),
                _ => Err(authority_error(format!(
                    "result {field} valid values must be nonempty canonical strings"
                ))),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(AuthorizedStringList::Valid),
        _ => Err(authority_error(format!(
            "result {field} status contradicts its values"
        ))),
    }
}

pub(super) fn decode(term: &Term, allowed: bool) -> Result<AuthorizedDatabasePolicy, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(legacy(None))
        } else {
            Err(authority_error(
                "denied result :database-policy must be nil",
            ))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error(
            "admitted result :database-policy must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [
        ":max-result-bytes",
        ":max-row-count",
        ":max-value-bytes",
        ":query-classes",
        ":target-allow",
    ]
    .into_iter()
    .map(|key| TermOrdKey(Term::symbol(key)))
    .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(
            "result :database-policy field set mismatch",
        ));
    }
    let field = |key: &str| {
        map.get(&TermOrdKey(Term::symbol(key)))
            .ok_or_else(|| authority_error(format!("result :database-policy is missing {key}")))
    };
    Ok(AuthorizedDatabasePolicy {
        target_allow: decode_string_list(field(":target-allow")?, ":target-allow")?,
        query_classes: decode_string_list(field(":query-classes")?, ":query-classes")?,
        max_result_bytes: decode_max_bytes_policy(field(":max-result-bytes")?, true)?,
        max_row_count: decode_max_bytes_policy(field(":max-row-count")?, true)?,
        max_value_bytes: decode_max_bytes_policy(field(":max-value-bytes")?, true)?,
    })
}
