use super::*;
use crate::policy::AuthorizedFfiPolicy;

pub(super) fn input(table: &toml::value::Table) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":abi-ids")),
                database::string_list_input(table.get("allow_abi_ids")),
            ),
            (
                TermOrdKey(Term::symbol(":evidence-mode")),
                network::optional_string_input(table.get("evidence_mode")),
            ),
            (
                TermOrdKey(Term::symbol(":libraries")),
                database::string_list_input(table.get("allow_libraries")),
            ),
            (
                TermOrdKey(Term::symbol(":max-buffer-bytes")),
                max_bytes_input(table.get("max_buffer_bytes")),
            ),
            (
                TermOrdKey(Term::symbol(":max-call-payload-bytes")),
                max_bytes_input(table.get("max_call_payload_bytes")),
            ),
            (
                TermOrdKey(Term::symbol(":policy-artifact-h")),
                network::optional_string_input(table.get("policy_artifact_h")),
            ),
            (
                TermOrdKey(Term::symbol(":policy-key-id")),
                network::optional_string_input(table.get("policy_key_id")),
            ),
            (
                TermOrdKey(Term::symbol(":policy-signature-h")),
                network::optional_string_input(table.get("policy_signature_h")),
            ),
            (
                TermOrdKey(Term::symbol(":schema-ids")),
                database::string_list_input(table.get("allow_schema_ids")),
            ),
            (
                TermOrdKey(Term::symbol(":symbols")),
                database::string_list_input(table.get("allow_symbols")),
            ),
            (
                TermOrdKey(Term::symbol(":signed-policy-required")),
                Term::Bool(
                    table
                        .get("signed_policy_required")
                        .and_then(toml::Value::as_bool)
                        .unwrap_or(false),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn legacy(policy: Option<&OpPolicy>) -> AuthorizedFfiPolicy {
    let extra = policy.map(|policy| &policy.extra);
    let get = |key| extra.and_then(|extra| extra.get(key));
    AuthorizedFfiPolicy {
        abi_ids: database::legacy_string_list(get("allow_abi_ids")),
        libraries: database::legacy_string_list(get("allow_libraries")),
        symbols: database::legacy_string_list(get("allow_symbols")),
        schema_ids: database::legacy_string_list(get("allow_schema_ids")),
        max_buffer_bytes: database::legacy_positive(get("max_buffer_bytes")),
        max_call_payload_bytes: database::legacy_positive(get("max_call_payload_bytes")),
        signed_policy_required: get("signed_policy_required")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        policy_artifact_h: network::legacy_optional_string(get("policy_artifact_h")),
        policy_signature_h: network::legacy_optional_string(get("policy_signature_h")),
        policy_key_id: network::legacy_optional_string(get("policy_key_id")),
        evidence_mode: network::legacy_optional_string(get("evidence_mode")),
    }
}

pub(super) fn decode(term: &Term, allowed: bool) -> Result<AuthorizedFfiPolicy, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(legacy(None))
        } else {
            Err(authority_error("denied result :ffi-policy must be nil"))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error(
            "admitted result :ffi-policy must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [
        ":abi-ids",
        ":evidence-mode",
        ":libraries",
        ":max-buffer-bytes",
        ":max-call-payload-bytes",
        ":policy-artifact-h",
        ":policy-key-id",
        ":policy-signature-h",
        ":schema-ids",
        ":signed-policy-required",
        ":symbols",
    ]
    .into_iter()
    .map(|key| TermOrdKey(Term::symbol(key)))
    .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result :ffi-policy field set mismatch"));
    }
    let field = |key: &str| {
        map.get(&TermOrdKey(Term::symbol(key)))
            .ok_or_else(|| authority_error(format!("result :ffi-policy is missing {key}")))
    };
    Ok(AuthorizedFfiPolicy {
        abi_ids: database::decode_string_list(field(":abi-ids")?, ":abi-ids")?,
        libraries: database::decode_string_list(field(":libraries")?, ":libraries")?,
        symbols: database::decode_string_list(field(":symbols")?, ":symbols")?,
        schema_ids: database::decode_string_list(field(":schema-ids")?, ":schema-ids")?,
        max_buffer_bytes: decode_max_bytes_policy(field(":max-buffer-bytes")?, true)?,
        max_call_payload_bytes: decode_max_bytes_policy(field(":max-call-payload-bytes")?, true)?,
        signed_policy_required: match field(":signed-policy-required")? {
            Term::Bool(value) => *value,
            _ => {
                return Err(authority_error(
                    "result :signed-policy-required must be boolean",
                ));
            }
        },
        policy_artifact_h: network::decode_optional_string(
            field(":policy-artifact-h")?,
            ":policy-artifact-h",
        )?,
        policy_signature_h: network::decode_optional_string(
            field(":policy-signature-h")?,
            ":policy-signature-h",
        )?,
        policy_key_id: network::decode_optional_string(field(":policy-key-id")?, ":policy-key-id")?,
        evidence_mode: network::decode_optional_string(field(":evidence-mode")?, ":evidence-mode")?,
    })
}
