use super::*;
use crate::policy::{
    AuthorizedBridgeAllowlist, AuthorizedBridgeDigest, AuthorizedBridgeIdentityPolicy,
    AuthorizedBridgeTransport,
};

pub(super) fn input(table: &toml::value::Table) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":allowlist")),
                database::string_list_input(table.get("bridge_cmd_allowlist")),
            ),
            (
                TermOrdKey(Term::symbol(":args")),
                database::string_list_input(table.get("bridge_args")),
            ),
            (
                TermOrdKey(Term::symbol(":command")),
                network::optional_string_input(table.get("bridge_cmd")),
            ),
            (
                TermOrdKey(Term::symbol(":transport")),
                network::optional_string_input(table.get("bridge_transport")),
            ),
            (
                TermOrdKey(Term::symbol(":digest")),
                network::optional_string_input(table.get("bridge_cmd_sha256")),
            ),
            (
                TermOrdKey(Term::symbol(":wasi-profile")),
                network::optional_bool_input(table.get("wasi_bridge_profile")),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn bridge_family_requires_pin(op: &str) -> bool {
    op.starts_with("host/plugin::") || op.starts_with("host/ffi::") || op.starts_with("editor/")
}

fn normalize_digest(raw: &str) -> Option<String> {
    let hex = raw
        .strip_prefix("sha256:")
        .or_else(|| raw.strip_prefix("SHA256:"))
        .unwrap_or(raw);
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex.to_ascii_lowercase())
}

pub(super) fn legacy(op: &str, policy: Option<&OpPolicy>) -> AuthorizedBridgeIdentityPolicy {
    let extra = policy.map(|policy| &policy.extra);
    let get = |key| extra.and_then(|extra| extra.get(key));
    let command = get("bridge_cmd")
        .and_then(toml::Value::as_str)
        .map(ToString::to_string);
    let args = get("bridge_args")
        .and_then(toml::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();
    let transport = match get("bridge_transport").and_then(toml::Value::as_str) {
        None => AuthorizedBridgeTransport::SpawnPerOp,
        Some(raw) => match raw.trim() {
            "" | "spawn-per-op" => AuthorizedBridgeTransport::SpawnPerOp,
            "persistent-stdio" => AuthorizedBridgeTransport::PersistentStdio,
            other => AuthorizedBridgeTransport::Invalid(other.to_string()),
        },
    };
    let wasi_profile = get("wasi_bridge_profile")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let allowlist = match get("bridge_cmd_allowlist") {
        None => AuthorizedBridgeAllowlist::Absent,
        Some(value) => match value.as_array() {
            None => AuthorizedBridgeAllowlist::InvalidType,
            Some(values) => {
                let mut normalized = Vec::with_capacity(values.len());
                let mut state = None;
                for value in values {
                    let Some(value) = value.as_str() else {
                        state = Some(AuthorizedBridgeAllowlist::InvalidEntry);
                        break;
                    };
                    let value = value.trim();
                    if value.is_empty() {
                        state = Some(AuthorizedBridgeAllowlist::EmptyEntry);
                        break;
                    }
                    normalized.push(value.to_string());
                }
                state.unwrap_or(AuthorizedBridgeAllowlist::Valid(normalized))
            }
        },
    };
    let digest = match network::legacy_optional_string(get("bridge_cmd_sha256")) {
        crate::policy::AuthorizedOptionalString::Absent => AuthorizedBridgeDigest::Absent,
        crate::policy::AuthorizedOptionalString::InvalidType => AuthorizedBridgeDigest::InvalidType,
        crate::policy::AuthorizedOptionalString::Empty => AuthorizedBridgeDigest::Empty,
        crate::policy::AuthorizedOptionalString::Valid(value) => normalize_digest(&value)
            .map(AuthorizedBridgeDigest::Valid)
            .unwrap_or(AuthorizedBridgeDigest::InvalidDigest),
    };
    AuthorizedBridgeIdentityPolicy {
        allowlist,
        args,
        command: command.clone(),
        pin_required: bridge_family_requires_pin(op)
            && command.is_some_and(|value| !value.trim().is_empty())
            && !wasi_profile,
        digest,
        transport,
        wasi_profile,
    }
}

fn decode_allowlist(term: &Term) -> Result<AuthorizedBridgeAllowlist, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error(
            "result :bridge-identity-policy :allowlist must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [":status", ":values"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(
            "result :bridge-identity-policy :allowlist field set mismatch",
        ));
    }
    let status = match map.get(&TermOrdKey(Term::symbol(":status"))) {
        Some(Term::Symbol(status)) => status.as_str(),
        _ => {
            return Err(authority_error(
                "result :bridge-identity-policy :allowlist :status must be a symbol",
            ));
        }
    };
    let values = map
        .get(&TermOrdKey(Term::symbol(":values")))
        .ok_or_else(|| {
            authority_error("result :bridge-identity-policy :allowlist is missing :values")
        })?;
    match (status, values) {
        (":absent", Term::Nil) => Ok(AuthorizedBridgeAllowlist::Absent),
        (":invalid-type", Term::Nil) => Ok(AuthorizedBridgeAllowlist::InvalidType),
        (":invalid-entry", Term::Nil) => Ok(AuthorizedBridgeAllowlist::InvalidEntry),
        (":empty-entry", Term::Nil) => Ok(AuthorizedBridgeAllowlist::EmptyEntry),
        (":valid", Term::Vector(values))
            if values.iter().all(|value| {
                matches!(value, Term::Str(value) if !value.is_empty() && value.trim() == value)
            }) =>
        {
            Ok(AuthorizedBridgeAllowlist::Valid(
                values
                    .iter()
                    .filter_map(|value| match value {
                        Term::Str(value) => Some(value.clone()),
                        _ => None,
                    })
                    .collect(),
            ))
        }
        _ => Err(authority_error(
            "result :bridge-identity-policy :allowlist status contradicts its values",
        )),
    }
}

fn decode_transport(term: &Term) -> Result<AuthorizedBridgeTransport, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error(
            "result :bridge-identity-policy :transport must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [":status", ":value"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(
            "result :bridge-identity-policy :transport field set mismatch",
        ));
    }
    let status = match map.get(&TermOrdKey(Term::symbol(":status"))) {
        Some(Term::Symbol(status)) => status.as_str(),
        _ => {
            return Err(authority_error(
                "result :bridge-identity-policy :transport :status must be a symbol",
            ));
        }
    };
    let value = map
        .get(&TermOrdKey(Term::symbol(":value")))
        .ok_or_else(|| {
            authority_error("result :bridge-identity-policy :transport is missing :value")
        })?;
    match (status, value) {
        (":spawn-per-op", Term::Nil) => Ok(AuthorizedBridgeTransport::SpawnPerOp),
        (":persistent-stdio", Term::Nil) => Ok(AuthorizedBridgeTransport::PersistentStdio),
        (":invalid", Term::Str(value))
            if !value.is_empty()
                && value.trim() == value
                && value != "spawn-per-op"
                && value != "persistent-stdio" =>
        {
            Ok(AuthorizedBridgeTransport::Invalid(value.clone()))
        }
        _ => Err(authority_error(
            "result :bridge-identity-policy :transport status contradicts its value",
        )),
    }
}

pub(super) fn decode(
    term: &Term,
    op: &str,
    allowed: bool,
) -> Result<AuthorizedBridgeIdentityPolicy, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(legacy(op, None))
        } else {
            Err(authority_error(
                "denied result :bridge-identity-policy must be nil",
            ))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error(
            "admitted result :bridge-identity-policy must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [
        ":allowlist",
        ":args",
        ":command",
        ":digest",
        ":pin-required",
        ":transport",
        ":wasi-profile",
    ]
    .into_iter()
    .map(|key| TermOrdKey(Term::symbol(key)))
    .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(
            "result :bridge-identity-policy field set mismatch",
        ));
    }
    let pin_required = match map.get(&TermOrdKey(Term::symbol(":pin-required"))) {
        Some(Term::Bool(value)) => *value,
        _ => {
            return Err(authority_error(
                "result :bridge-identity-policy :pin-required must be bool",
            ));
        }
    };
    if pin_required && !bridge_family_requires_pin(op) {
        return Err(authority_error(
            "result :bridge-identity-policy requires a pin for an ineligible operation",
        ));
    }
    let allowlist = decode_allowlist(
        map.get(&TermOrdKey(Term::symbol(":allowlist")))
            .ok_or_else(|| {
                authority_error("result :bridge-identity-policy is missing :allowlist")
            })?,
    )?;
    let args = match map.get(&TermOrdKey(Term::symbol(":args"))) {
        Some(Term::Vector(values)) if values.iter().all(|value| matches!(value, Term::Str(_))) => {
            values
                .iter()
                .filter_map(|value| match value {
                    Term::Str(value) => Some(value.clone()),
                    _ => None,
                })
                .collect()
        }
        _ => {
            return Err(authority_error(
                "result :bridge-identity-policy :args must be a string vector",
            ));
        }
    };
    let command = match map.get(&TermOrdKey(Term::symbol(":command"))) {
        Some(Term::Nil) => None,
        Some(Term::Str(value)) => Some(value.clone()),
        _ => {
            return Err(authority_error(
                "result :bridge-identity-policy :command must be nil or string",
            ));
        }
    };
    let digest_term = map
        .get(&TermOrdKey(Term::symbol(":digest")))
        .ok_or_else(|| authority_error("result :bridge-identity-policy is missing :digest"))?;
    let Term::Map(digest_map) = digest_term else {
        return Err(authority_error(
            "result :bridge-identity-policy :digest must be a data map",
        ));
    };
    let digest_expected: BTreeSet<_> = [":status", ":value"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if digest_map.keys().cloned().collect::<BTreeSet<_>>() != digest_expected {
        return Err(authority_error(
            "result :bridge-identity-policy :digest field set mismatch",
        ));
    }
    let status = match digest_map.get(&TermOrdKey(Term::symbol(":status"))) {
        Some(Term::Symbol(status)) => status.as_str(),
        _ => {
            return Err(authority_error(
                "result :bridge-identity-policy :digest :status must be a symbol",
            ));
        }
    };
    let value = digest_map
        .get(&TermOrdKey(Term::symbol(":value")))
        .ok_or_else(|| {
            authority_error("result :bridge-identity-policy :digest is missing :value")
        })?;
    let digest = match (status, value) {
        (":absent", Term::Nil) => AuthorizedBridgeDigest::Absent,
        (":invalid-type", Term::Nil) => AuthorizedBridgeDigest::InvalidType,
        (":empty", Term::Nil) => AuthorizedBridgeDigest::Empty,
        (":invalid-digest", Term::Nil) => AuthorizedBridgeDigest::InvalidDigest,
        (":valid", Term::Str(value))
            if value.len() == 64
                && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                && value.bytes().all(|byte| !byte.is_ascii_uppercase()) =>
        {
            AuthorizedBridgeDigest::Valid(value.clone())
        }
        _ => {
            return Err(authority_error(
                "result :bridge-identity-policy :digest status contradicts its value",
            ));
        }
    };
    let transport = decode_transport(
        map.get(&TermOrdKey(Term::symbol(":transport")))
            .ok_or_else(|| {
                authority_error("result :bridge-identity-policy is missing :transport")
            })?,
    )?;
    let wasi_profile = match map.get(&TermOrdKey(Term::symbol(":wasi-profile"))) {
        Some(Term::Bool(value)) => *value,
        _ => {
            return Err(authority_error(
                "result :bridge-identity-policy :wasi-profile must be bool",
            ));
        }
    };
    Ok(AuthorizedBridgeIdentityPolicy {
        allowlist,
        args,
        command,
        pin_required,
        digest,
        transport,
        wasi_profile,
    })
}
