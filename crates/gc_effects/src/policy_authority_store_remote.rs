use super::*;
use crate::policy::AuthorizedStoreRemotePolicy;

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
