use super::*;
use crate::policy::{
    AuthorizedOptionalBool, AuthorizedPositiveI64, AuthorizedStringList, AuthorizedXrBackend,
};

pub(super) fn backend(
    backend: Option<&Term>,
    invalid: Option<&Term>,
) -> Result<AuthorizedXrBackend, EffectsError> {
    let invalid =
        invalid.ok_or_else(|| authority_error("result :xr-policy is missing :invalid-value"))?;
    match backend {
        Some(Term::Symbol(value)) if value == ":first-party-runtime" && invalid == &Term::Nil => {
            Ok(AuthorizedXrBackend::FirstParty)
        }
        Some(Term::Symbol(value)) if value == ":webxr-device" && invalid == &Term::Nil => {
            Ok(AuthorizedXrBackend::WebxrDevice)
        }
        Some(Term::Symbol(value))
            if value == ":production-requires-bridge" && invalid == &Term::Nil =>
        {
            Ok(AuthorizedXrBackend::ProductionRequiresBridge)
        }
        Some(Term::Symbol(value)) if value == ":invalid" => match invalid {
            Term::Str(value) if !value.is_empty() => {
                Ok(AuthorizedXrBackend::Invalid(value.clone()))
            }
            _ => Err(authority_error(
                "invalid XR backend decision must carry a nonempty string",
            )),
        },
        _ => Err(authority_error(
            "contradictory result :xr-policy backend state",
        )),
    }
}

pub(super) fn bool_field(term: &Term, field: &str) -> Result<AuthorizedOptionalBool, EffectsError> {
    match term {
        Term::Nil => Ok(AuthorizedOptionalBool::Absent),
        Term::Symbol(status) if status == ":invalid-type" => {
            Ok(AuthorizedOptionalBool::InvalidType)
        }
        Term::Bool(value) => Ok(AuthorizedOptionalBool::Valid(*value)),
        _ => Err(authority_error(format!(
            "result {field} must be nil, :invalid-type, or a boolean"
        ))),
    }
}

pub(super) fn positive_i64(
    term: &Term,
    field: &str,
    permit_out_of_range: bool,
) -> Result<AuthorizedPositiveI64, EffectsError> {
    match term {
        Term::Nil => Ok(AuthorizedPositiveI64::Absent),
        Term::Symbol(status) if status == ":invalid-type" => Ok(AuthorizedPositiveI64::InvalidType),
        Term::Symbol(status) if status == ":nonpositive" => Ok(AuthorizedPositiveI64::NonPositive),
        Term::Symbol(status) if permit_out_of_range && status == ":out-of-range" => {
            Ok(AuthorizedPositiveI64::OutOfRange)
        }
        Term::Int(value) if value.to_i64().is_some_and(|value| value > 0) => {
            Ok(AuthorizedPositiveI64::Valid(value.to_i64().unwrap_or(1)))
        }
        _ => Err(authority_error(format!(
            "result {field} is not a canonical positive-integer decision"
        ))),
    }
}

pub(super) fn string_list(
    term: &Term,
    field: &str,
    lowercase: bool,
) -> Result<AuthorizedStringList, EffectsError> {
    let values = match term {
        Term::Nil => return Ok(AuthorizedStringList::Absent),
        Term::Symbol(status) if status == ":invalid-type" => {
            return Ok(AuthorizedStringList::InvalidType);
        }
        Term::Symbol(status) if status == ":invalid-entry" => {
            return Ok(AuthorizedStringList::InvalidEntry);
        }
        Term::Symbol(status) if status == ":empty" => return Ok(AuthorizedStringList::Empty),
        Term::Vector(values) if !values.is_empty() => values,
        _ => {
            return Err(authority_error(format!(
                "result {field} must be nil, a closed status, or a nonempty vector"
            )));
        }
    };
    let values = values
        .iter()
        .map(|value| match value {
            Term::Str(value)
                if !value.is_empty()
                    && value.trim() == value
                    && (!lowercase || value.to_ascii_lowercase() == *value) =>
            {
                Ok(value.clone())
            }
            _ => Err(authority_error(format!(
                "result {field} contains a noncanonical string"
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AuthorizedStringList::Valid(values))
}
