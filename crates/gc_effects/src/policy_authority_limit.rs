use super::*;
use crate::policy::AuthorizedMaxBytes;

pub(super) fn legacy(policy: Option<&OpPolicy>) -> AuthorizedMaxBytes {
    let Some(value) = policy.and_then(|policy| policy.extra.get("max_bytes")) else {
        return AuthorizedMaxBytes::Absent;
    };
    let Some(raw) = value.as_integer() else {
        return AuthorizedMaxBytes::InvalidType;
    };
    if raw <= 0 {
        return AuthorizedMaxBytes::NonPositive;
    }
    match usize::try_from(raw) {
        Ok(limit) => AuthorizedMaxBytes::Valid(limit),
        Err(_) => AuthorizedMaxBytes::PlatformOverflow,
    }
}

pub(super) fn decode(term: &Term, allowed: bool) -> Result<AuthorizedMaxBytes, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(AuthorizedMaxBytes::Absent)
        } else {
            Err(authority_error(
                "denied result :max-bytes-policy must be nil",
            ))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error(
            "admitted result :max-bytes-policy must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [":limit", ":status"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(
            "result :max-bytes-policy field set mismatch",
        ));
    }
    let status = match map.get(&TermOrdKey(Term::symbol(":status"))) {
        Some(Term::Symbol(status)) => status.as_str(),
        _ => {
            return Err(authority_error(
                "result :max-bytes-policy :status must be a symbol",
            ));
        }
    };
    let limit = map
        .get(&TermOrdKey(Term::symbol(":limit")))
        .ok_or_else(|| authority_error("result :max-bytes-policy is missing :limit"))?;
    match (status, limit) {
        (":absent", Term::Nil) => Ok(AuthorizedMaxBytes::Absent),
        (":invalid-type", Term::Nil) => Ok(AuthorizedMaxBytes::InvalidType),
        (":nonpositive", Term::Nil) => Ok(AuthorizedMaxBytes::NonPositive),
        (":platform-overflow", Term::Nil) => Ok(AuthorizedMaxBytes::PlatformOverflow),
        (":valid", Term::Int(value)) => value
            .to_usize()
            .filter(|limit| *limit > 0)
            .map(AuthorizedMaxBytes::Valid)
            .ok_or_else(|| {
                authority_error(
                    "result :max-bytes-policy valid limit must fit a positive platform usize",
                )
            }),
        _ => Err(authority_error(
            "result :max-bytes-policy status contradicts its limit",
        )),
    }
}
