use super::*;
use crate::policy::{AuthorizedBridgeDigest, AuthorizedBridgeIdentityPolicy};

pub(super) fn input(table: &toml::value::Table) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":command")),
                network::optional_string_input(table.get("bridge_cmd")),
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
    let command_present = get("bridge_cmd")
        .and_then(toml::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let wasi_profile = get("wasi_bridge_profile")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let digest = match network::legacy_optional_string(get("bridge_cmd_sha256")) {
        crate::policy::AuthorizedOptionalString::Absent => AuthorizedBridgeDigest::Absent,
        crate::policy::AuthorizedOptionalString::InvalidType => AuthorizedBridgeDigest::InvalidType,
        crate::policy::AuthorizedOptionalString::Empty => AuthorizedBridgeDigest::Empty,
        crate::policy::AuthorizedOptionalString::Valid(value) => normalize_digest(&value)
            .map(AuthorizedBridgeDigest::Valid)
            .unwrap_or(AuthorizedBridgeDigest::InvalidDigest),
    };
    AuthorizedBridgeIdentityPolicy {
        pin_required: bridge_family_requires_pin(op) && command_present && !wasi_profile,
        digest,
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
    let expected: BTreeSet<_> = [":digest", ":pin-required"]
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
    Ok(AuthorizedBridgeIdentityPolicy {
        pin_required,
        digest,
    })
}
