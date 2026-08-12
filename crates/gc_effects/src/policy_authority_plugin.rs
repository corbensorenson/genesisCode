use super::*;
use crate::policy::AuthorizedPluginPolicy;

pub(super) fn input(table: &toml::value::Table) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":commands")),
                database::string_list_input(table.get("allow_commands")),
            ),
            (
                TermOrdKey(Term::symbol(":plugins")),
                database::string_list_input(table.get("allow_plugins")),
            ),
            (
                TermOrdKey(Term::symbol(":schema-ids")),
                database::string_list_input(table.get("allow_schema_ids")),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn legacy(policy: Option<&OpPolicy>) -> AuthorizedPluginPolicy {
    let extra = policy.map(|policy| &policy.extra);
    let get = |key| extra.and_then(|extra| extra.get(key));
    AuthorizedPluginPolicy {
        plugins: database::legacy_string_list(get("allow_plugins")),
        commands: database::legacy_string_list(get("allow_commands")),
        schema_ids: database::legacy_string_list(get("allow_schema_ids")),
    }
}

pub(super) fn decode(term: &Term, allowed: bool) -> Result<AuthorizedPluginPolicy, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(legacy(None))
        } else {
            Err(authority_error("denied result :plugin-policy must be nil"))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error(
            "admitted result :plugin-policy must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [":commands", ":plugins", ":schema-ids"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result :plugin-policy field set mismatch"));
    }
    let field = |key: &str| {
        map.get(&TermOrdKey(Term::symbol(key)))
            .ok_or_else(|| authority_error(format!("result :plugin-policy is missing {key}")))
    };
    Ok(AuthorizedPluginPolicy {
        plugins: database::decode_string_list(field(":plugins")?, ":plugins")?,
        commands: database::decode_string_list(field(":commands")?, ":commands")?,
        schema_ids: database::decode_string_list(field(":schema-ids")?, ":schema-ids")?,
    })
}
