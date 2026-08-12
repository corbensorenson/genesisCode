use super::*;
use crate::policy::{
    AuthorizedOptionalBool, AuthorizedOptionalString, AuthorizedStoreRemotePolicy,
    AuthorizedStringList,
};

pub(super) fn input(store: Option<&toml::value::Table>) -> Term {
    let get = |key| store.and_then(|table| table.get(key));
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":allow-http")),
                network::optional_bool_input(get("allow_http")),
            ),
            (
                TermOrdKey(Term::symbol(":remote")),
                network::optional_string_input(get("remote")),
            ),
            (
                TermOrdKey(Term::symbol(":remote-allow")),
                database::string_list_input(get("remote_allow")),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn legacy(store: Option<&toml::value::Table>) -> AuthorizedStoreRemotePolicy {
    let get = |key| store.and_then(|table| table.get(key));
    AuthorizedStoreRemotePolicy {
        remote: network::legacy_optional_string(get("remote")),
        remote_allow: database::legacy_string_list(get("remote_allow")),
        allow_http: network::legacy_optional_bool(get("allow_http")),
    }
}

pub(super) fn decode(term: &Term) -> Result<AuthorizedStoreRemotePolicy, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error(
            "resource result :store :remote-policy must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [":allow-http", ":remote", ":remote-allow"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(
            "resource result :store :remote-policy field set mismatch",
        ));
    }
    let field = |key: &str| {
        map.get(&TermOrdKey(Term::symbol(key))).ok_or_else(|| {
            authority_error(format!(
                "resource result :store :remote-policy is missing {key}"
            ))
        })
    };
    Ok(AuthorizedStoreRemotePolicy {
        remote: network::decode_optional_string(field(":remote")?, ":store :remote")?,
        remote_allow: database::decode_string_list(
            field(":remote-allow")?,
            ":store :remote-allow",
        )?,
        allow_http: network::decode_optional_bool(field(":allow-http")?)?,
    })
}

pub(super) fn term(policy: &AuthorizedStoreRemotePolicy) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":allow-http")),
                optional_bool_term(&policy.allow_http),
            ),
            (
                TermOrdKey(Term::symbol(":remote")),
                optional_string_term(&policy.remote),
            ),
            (
                TermOrdKey(Term::symbol(":remote-allow")),
                string_list_term(&policy.remote_allow),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn optional_bool_term(value: &AuthorizedOptionalBool) -> Term {
    let (status, value) = match value {
        AuthorizedOptionalBool::Absent => (":absent", Term::Nil),
        AuthorizedOptionalBool::InvalidType => (":invalid-type", Term::Nil),
        AuthorizedOptionalBool::Valid(value) => (":valid", Term::Bool(*value)),
    };
    state_term(status, ":value", value)
}

fn optional_string_term(value: &AuthorizedOptionalString) -> Term {
    let (status, value) = match value {
        AuthorizedOptionalString::Absent => (":absent", Term::Nil),
        AuthorizedOptionalString::InvalidType => (":invalid-type", Term::Nil),
        AuthorizedOptionalString::Empty => (":empty", Term::Nil),
        AuthorizedOptionalString::Valid(value) => (":valid", Term::Str(value.clone())),
    };
    state_term(status, ":value", value)
}

fn string_list_term(value: &AuthorizedStringList) -> Term {
    let (status, values) = match value {
        AuthorizedStringList::Absent => (":absent", Term::Nil),
        AuthorizedStringList::InvalidType => (":invalid-type", Term::Nil),
        AuthorizedStringList::InvalidEntry => (":invalid-entry", Term::Nil),
        AuthorizedStringList::Empty => (":empty", Term::Nil),
        AuthorizedStringList::Valid(values) => (
            ":valid",
            Term::Vector(values.iter().cloned().map(Term::Str).collect()),
        ),
    };
    state_term(status, ":values", values)
}

fn state_term(status: &str, value_key: &str, value: Term) -> Term {
    Term::Map(
        [
            (TermOrdKey(Term::symbol(":status")), Term::symbol(status)),
            (TermOrdKey(Term::symbol(value_key)), value),
        ]
        .into_iter()
        .collect(),
    )
}
