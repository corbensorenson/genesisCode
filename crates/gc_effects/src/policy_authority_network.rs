use super::*;
use crate::policy::{
    AuthorizedBindPorts, AuthorizedNetworkPolicy, AuthorizedOptionalBool, AuthorizedOptionalString,
};

fn string_list_input(value: Option<&toml::Value>) -> Term {
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

fn optional_bool_input(value: Option<&toml::Value>) -> Term {
    match value {
        None => Term::Nil,
        Some(value) => value
            .as_bool()
            .map(Term::Bool)
            .unwrap_or_else(|| Term::symbol(":invalid-type")),
    }
}

pub(super) fn optional_string_input(value: Option<&toml::Value>) -> Term {
    match value {
        None => Term::Nil,
        Some(value) => value
            .as_str()
            .map(|value| Term::Str(value.to_string()))
            .unwrap_or_else(|| Term::symbol(":invalid-type")),
    }
}

fn bind_ports_input(value: Option<&toml::Value>) -> Term {
    match value {
        None => Term::Nil,
        Some(value) => match value.as_array() {
            Some(values) => Term::Vector(
                values
                    .iter()
                    .map(|value| {
                        value
                            .as_integer()
                            .map(|value| Term::Int(value.into()))
                            .or_else(|| value.as_str().map(|value| Term::Str(value.to_string())))
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
                TermOrdKey(Term::symbol(":allow-http")),
                optional_bool_input(table.get("allow_http")),
            ),
            (
                TermOrdKey(Term::symbol(":bind-hosts")),
                string_list_input(table.get("allow_bind_hosts")),
            ),
            (
                TermOrdKey(Term::symbol(":bind-ports")),
                bind_ports_input(table.get("allow_bind_ports")),
            ),
            (
                TermOrdKey(Term::symbol(":max-request-bytes")),
                max_bytes_input(table.get("max_request_bytes")),
            ),
            (
                TermOrdKey(Term::symbol(":remote-allow")),
                string_list_input(table.get("remote_allow")),
            ),
            (
                TermOrdKey(Term::symbol(":url-allow")),
                string_list_input(table.get("url_allow")),
            ),
            (
                TermOrdKey(Term::symbol(":wasi-network-profile")),
                optional_string_input(table.get("wasi_network_profile")),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn legacy_optional_bool(value: Option<&toml::Value>) -> AuthorizedOptionalBool {
    match value {
        None => AuthorizedOptionalBool::Absent,
        Some(value) => value
            .as_bool()
            .map(AuthorizedOptionalBool::Valid)
            .unwrap_or(AuthorizedOptionalBool::InvalidType),
    }
}

pub(super) fn legacy_optional_string(value: Option<&toml::Value>) -> AuthorizedOptionalString {
    match value {
        None => AuthorizedOptionalString::Absent,
        Some(value) => match value.as_str() {
            None => AuthorizedOptionalString::InvalidType,
            Some(value) => {
                let value = value.trim();
                if value.is_empty() {
                    AuthorizedOptionalString::Empty
                } else {
                    AuthorizedOptionalString::Valid(value.to_string())
                }
            }
        },
    }
}

fn legacy_bind_ports(value: Option<&toml::Value>) -> AuthorizedBindPorts {
    let Some(value) = value else {
        return AuthorizedBindPorts::Absent;
    };
    let Some(values) = value.as_array() else {
        return AuthorizedBindPorts::InvalidType;
    };
    let mut any = false;
    let mut ports = Vec::new();
    for value in values {
        if let Some(port) = value.as_integer() {
            if !(1..=65_535).contains(&port) {
                return AuthorizedBindPorts::OutOfRange;
            }
            ports.push(port as u16);
        } else if value.as_str().is_some_and(|value| value.trim() == "*") {
            any = true;
        } else {
            return AuthorizedBindPorts::InvalidEntry;
        }
    }
    if !any && ports.is_empty() {
        AuthorizedBindPorts::Empty
    } else {
        AuthorizedBindPorts::Valid { any, ports }
    }
}

pub(super) fn legacy(policy: Option<&OpPolicy>) -> AuthorizedNetworkPolicy {
    let extra = policy.map(|policy| &policy.extra);
    let get = |key| extra.and_then(|extra| extra.get(key));
    AuthorizedNetworkPolicy {
        url_allow: database::legacy_string_list(get("url_allow")),
        remote_allow: database::legacy_string_list(get("remote_allow")),
        allow_http: legacy_optional_bool(get("allow_http")),
        wasi_network_profile: legacy_optional_string(get("wasi_network_profile")),
        bind_hosts: database::legacy_string_list(get("allow_bind_hosts")),
        bind_ports: legacy_bind_ports(get("allow_bind_ports")),
        max_request_bytes: database::legacy_positive(get("max_request_bytes")),
    }
}

fn decode_optional_bool(term: &Term) -> Result<AuthorizedOptionalBool, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error("result :allow-http must be a data map"));
    };
    let expected: BTreeSet<_> = [":status", ":value"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result :allow-http field set mismatch"));
    }
    match (
        map.get(&TermOrdKey(Term::symbol(":status"))),
        map.get(&TermOrdKey(Term::symbol(":value"))),
    ) {
        (Some(Term::Symbol(status)), Some(Term::Nil)) if status == ":absent" => {
            Ok(AuthorizedOptionalBool::Absent)
        }
        (Some(Term::Symbol(status)), Some(Term::Nil)) if status == ":invalid-type" => {
            Ok(AuthorizedOptionalBool::InvalidType)
        }
        (Some(Term::Symbol(status)), Some(Term::Bool(value))) if status == ":valid" => {
            Ok(AuthorizedOptionalBool::Valid(*value))
        }
        _ => Err(authority_error(
            "result :allow-http status contradicts its value",
        )),
    }
}

pub(super) fn decode_optional_string(
    term: &Term,
    field: &str,
) -> Result<AuthorizedOptionalString, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error(format!(
            "result {field} must be a data map"
        )));
    };
    let expected: BTreeSet<_> = [":status", ":value"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(format!(
            "result {field} field set mismatch"
        )));
    }
    match (
        map.get(&TermOrdKey(Term::symbol(":status"))),
        map.get(&TermOrdKey(Term::symbol(":value"))),
    ) {
        (Some(Term::Symbol(status)), Some(Term::Nil)) if status == ":absent" => {
            Ok(AuthorizedOptionalString::Absent)
        }
        (Some(Term::Symbol(status)), Some(Term::Nil)) if status == ":invalid-type" => {
            Ok(AuthorizedOptionalString::InvalidType)
        }
        (Some(Term::Symbol(status)), Some(Term::Nil)) if status == ":empty" => {
            Ok(AuthorizedOptionalString::Empty)
        }
        (Some(Term::Symbol(status)), Some(Term::Str(value)))
            if status == ":valid" && !value.is_empty() && value.trim() == value =>
        {
            Ok(AuthorizedOptionalString::Valid(value.clone()))
        }
        _ => Err(authority_error(format!(
            "result {field} status contradicts its value"
        ))),
    }
}

fn decode_bind_ports(term: &Term) -> Result<AuthorizedBindPorts, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error("result :bind-ports must be a data map"));
    };
    let expected: BTreeSet<_> = [":any", ":ports", ":status"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result :bind-ports field set mismatch"));
    }
    let status = match map.get(&TermOrdKey(Term::symbol(":status"))) {
        Some(Term::Symbol(status)) => status.as_str(),
        _ => {
            return Err(authority_error(
                "result :bind-ports status must be a symbol",
            ));
        }
    };
    let any = map.get(&TermOrdKey(Term::symbol(":any")));
    let ports = map.get(&TermOrdKey(Term::symbol(":ports")));
    let empty_state = match status {
        ":absent" => Some(AuthorizedBindPorts::Absent),
        ":invalid-type" => Some(AuthorizedBindPorts::InvalidType),
        ":invalid-entry" => Some(AuthorizedBindPorts::InvalidEntry),
        ":out-of-range" => Some(AuthorizedBindPorts::OutOfRange),
        ":empty" => Some(AuthorizedBindPorts::Empty),
        _ => None,
    };
    if let Some(state) = empty_state {
        return if any == Some(&Term::Nil) && ports == Some(&Term::Nil) {
            Ok(state)
        } else {
            Err(authority_error(
                "result :bind-ports nonvalid status must carry nil fields",
            ))
        };
    }
    let (Some(Term::Bool(any)), Some(Term::Vector(values))) = (any, ports) else {
        return Err(authority_error(
            "result :bind-ports valid status requires bool and vector fields",
        ));
    };
    if status != ":valid" || (!*any && values.is_empty()) {
        return Err(authority_error(
            "result :bind-ports status contradicts its fields",
        ));
    }
    let ports = values
        .iter()
        .map(|value| match value {
            Term::Int(value) => value
                .to_u16()
                .filter(|value| *value > 0)
                .ok_or_else(|| authority_error("result :bind-ports values must be 1..65535")),
            _ => Err(authority_error(
                "result :bind-ports values must be integers",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AuthorizedBindPorts::Valid { any: *any, ports })
}

pub(super) fn decode(term: &Term, allowed: bool) -> Result<AuthorizedNetworkPolicy, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(legacy(None))
        } else {
            Err(authority_error("denied result :network-policy must be nil"))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error(
            "admitted result :network-policy must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [
        ":allow-http",
        ":bind-hosts",
        ":bind-ports",
        ":max-request-bytes",
        ":remote-allow",
        ":url-allow",
        ":wasi-network-profile",
    ]
    .into_iter()
    .map(|key| TermOrdKey(Term::symbol(key)))
    .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result :network-policy field set mismatch"));
    }
    let field = |key: &str| {
        map.get(&TermOrdKey(Term::symbol(key)))
            .ok_or_else(|| authority_error(format!("result :network-policy is missing {key}")))
    };
    Ok(AuthorizedNetworkPolicy {
        url_allow: database::decode_string_list(field(":url-allow")?, ":url-allow")?,
        remote_allow: database::decode_string_list(field(":remote-allow")?, ":remote-allow")?,
        allow_http: decode_optional_bool(field(":allow-http")?)?,
        wasi_network_profile: decode_optional_string(
            field(":wasi-network-profile")?,
            ":wasi-network-profile",
        )?,
        bind_hosts: database::decode_string_list(field(":bind-hosts")?, ":bind-hosts")?,
        bind_ports: decode_bind_ports(field(":bind-ports")?)?,
        max_request_bytes: decode_max_bytes_policy(field(":max-request-bytes")?, true)?,
    })
}
