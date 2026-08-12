use super::*;
use crate::policy::{AuthorizedFfiPolicy, AuthorizedFfiSignedPolicy, AuthorizedOptionalString};

fn signed_policy_required_input(value: Option<&toml::Value>) -> Term {
    match value {
        None => Term::Bool(false),
        Some(toml::Value::Boolean(value)) => Term::Bool(*value),
        Some(_) => Term::symbol(":invalid-type"),
    }
}

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
                signed_policy_required_input(table.get("signed_policy_required")),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn signed_string_state(
    state: &AuthorizedOptionalString,
    missing: AuthorizedFfiSignedPolicy,
    empty: AuthorizedFfiSignedPolicy,
) -> Result<String, AuthorizedFfiSignedPolicy> {
    match state {
        AuthorizedOptionalString::Absent | AuthorizedOptionalString::InvalidType => Err(missing),
        AuthorizedOptionalString::Empty => Err(empty),
        AuthorizedOptionalString::Valid(value) => Ok(value.clone()),
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn legacy_signed_policy(
    extra: Option<&BTreeMap<String, toml::Value>>,
) -> AuthorizedFfiSignedPolicy {
    let get = |key| extra.and_then(|extra| extra.get(key));
    match get("signed_policy_required") {
        None | Some(toml::Value::Boolean(false)) => return AuthorizedFfiSignedPolicy::Disabled,
        Some(toml::Value::Boolean(true)) => {}
        Some(_) => return AuthorizedFfiSignedPolicy::InvalidRequiredType,
    }
    let artifact = match signed_string_state(
        &network::legacy_optional_string(get("policy_artifact_h")),
        AuthorizedFfiSignedPolicy::MissingArtifactHash,
        AuthorizedFfiSignedPolicy::EmptyArtifactHash,
    ) {
        Ok(value) => value,
        Err(state) => return state,
    };
    if !is_hex64(&artifact) {
        return AuthorizedFfiSignedPolicy::InvalidArtifactHash;
    }
    let signature = match signed_string_state(
        &network::legacy_optional_string(get("policy_signature_h")),
        AuthorizedFfiSignedPolicy::MissingSignatureHash,
        AuthorizedFfiSignedPolicy::EmptySignatureHash,
    ) {
        Ok(value) => value,
        Err(state) => return state,
    };
    if !is_hex64(&signature) {
        return AuthorizedFfiSignedPolicy::InvalidSignatureHash;
    }
    let key_id = match signed_string_state(
        &network::legacy_optional_string(get("policy_key_id")),
        AuthorizedFfiSignedPolicy::MissingKeyId,
        AuthorizedFfiSignedPolicy::EmptyKeyId,
    ) {
        Ok(value) => value,
        Err(state) => return state,
    };
    let evidence_mode = match signed_string_state(
        &network::legacy_optional_string(get("evidence_mode")),
        AuthorizedFfiSignedPolicy::MissingEvidenceMode,
        AuthorizedFfiSignedPolicy::EmptyEvidenceMode,
    ) {
        Ok(value) => value,
        Err(state) => return state,
    };
    if evidence_mode != "deterministic" {
        return AuthorizedFfiSignedPolicy::InvalidEvidenceMode;
    }
    AuthorizedFfiSignedPolicy::Valid {
        policy_artifact_h: artifact,
        policy_signature_h: signature,
        policy_key_id: key_id,
        evidence_mode,
    }
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
        signed_policy: legacy_signed_policy(extra),
    }
}

fn decode_signed_policy(term: &Term) -> Result<AuthorizedFfiSignedPolicy, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error("result :signed-policy must be a data map"));
    };
    let expected: BTreeSet<_> = [
        ":evidence-mode",
        ":policy-artifact-h",
        ":policy-key-id",
        ":policy-signature-h",
        ":status",
    ]
    .into_iter()
    .map(|key| TermOrdKey(Term::symbol(key)))
    .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result :signed-policy field set mismatch"));
    }
    let field = |key: &str| {
        map.get(&TermOrdKey(Term::symbol(key)))
            .ok_or_else(|| authority_error(format!("result :signed-policy is missing {key}")))
    };
    let status = match field(":status")? {
        Term::Symbol(status) => status.as_str(),
        _ => {
            return Err(authority_error(
                "result :signed-policy :status must be a symbol",
            ));
        }
    };
    let artifact = field(":policy-artifact-h")?;
    let signature = field(":policy-signature-h")?;
    let key_id = field(":policy-key-id")?;
    let evidence_mode = field(":evidence-mode")?;
    let nil_payload = artifact == &Term::Nil
        && signature == &Term::Nil
        && key_id == &Term::Nil
        && evidence_mode == &Term::Nil;
    let closed_error = |decision| {
        if nil_payload {
            Ok(decision)
        } else {
            Err(authority_error(
                "result :signed-policy error status must carry nil metadata",
            ))
        }
    };
    match status {
        ":disabled" => closed_error(AuthorizedFfiSignedPolicy::Disabled),
        ":invalid-required-type" => closed_error(AuthorizedFfiSignedPolicy::InvalidRequiredType),
        ":missing-artifact-h" => closed_error(AuthorizedFfiSignedPolicy::MissingArtifactHash),
        ":empty-artifact-h" => closed_error(AuthorizedFfiSignedPolicy::EmptyArtifactHash),
        ":invalid-artifact-h" => closed_error(AuthorizedFfiSignedPolicy::InvalidArtifactHash),
        ":missing-signature-h" => closed_error(AuthorizedFfiSignedPolicy::MissingSignatureHash),
        ":empty-signature-h" => closed_error(AuthorizedFfiSignedPolicy::EmptySignatureHash),
        ":invalid-signature-h" => closed_error(AuthorizedFfiSignedPolicy::InvalidSignatureHash),
        ":missing-key-id" => closed_error(AuthorizedFfiSignedPolicy::MissingKeyId),
        ":empty-key-id" => closed_error(AuthorizedFfiSignedPolicy::EmptyKeyId),
        ":missing-evidence-mode" => closed_error(AuthorizedFfiSignedPolicy::MissingEvidenceMode),
        ":empty-evidence-mode" => closed_error(AuthorizedFfiSignedPolicy::EmptyEvidenceMode),
        ":invalid-evidence-mode" => closed_error(AuthorizedFfiSignedPolicy::InvalidEvidenceMode),
        ":valid" => match (artifact, signature, key_id, evidence_mode) {
            (Term::Str(artifact), Term::Str(signature), Term::Str(key_id), Term::Str(mode))
                if is_hex64(artifact)
                    && is_hex64(signature)
                    && !key_id.is_empty()
                    && key_id.trim() == key_id
                    && mode == "deterministic" =>
            {
                Ok(AuthorizedFfiSignedPolicy::Valid {
                    policy_artifact_h: artifact.clone(),
                    policy_signature_h: signature.clone(),
                    policy_key_id: key_id.clone(),
                    evidence_mode: mode.clone(),
                })
            }
            _ => Err(authority_error(
                "result :signed-policy valid status contradicts its metadata",
            )),
        },
        _ => Err(authority_error("result :signed-policy has unknown status")),
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
        ":libraries",
        ":max-buffer-bytes",
        ":max-call-payload-bytes",
        ":schema-ids",
        ":signed-policy",
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
        signed_policy: decode_signed_policy(field(":signed-policy")?)?,
    })
}
