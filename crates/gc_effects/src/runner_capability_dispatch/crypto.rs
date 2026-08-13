use super::*;
#[cfg(test)]
use crate::policy::AuthorizedCryptoPolicy;
use crate::policy::{AuthorizedMaxBytes, AuthorizedStringList};

fn payload_required_bytes_field(
    payload: &Term,
    op: &str,
    key: &str,
) -> Result<Vec<u8>, EffectsError> {
    let value = payload_required_field(payload, op, key)?;
    match value {
        Term::Bytes(bytes) => Ok(bytes.to_vec()),
        Term::Str(text) => Ok(text.into_bytes()),
        _ => Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must be bytes or string"
        ))),
    }
}

fn payload_optional_bytes_field(
    payload: &Term,
    op: &str,
    key: &str,
) -> Result<Option<Vec<u8>>, EffectsError> {
    let Some(value) = payload_optional_field(payload, op, key)? else {
        return Ok(None);
    };
    match value {
        Term::Bytes(bytes) => Ok(Some(bytes.to_vec())),
        Term::Str(text) => Ok(Some(text.into_bytes())),
        _ => Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must be bytes or string"
        ))),
    }
}

fn payload_required_positive_usize_field(
    payload: &Term,
    op: &str,
    key: &str,
) -> Result<usize, EffectsError> {
    let value = payload_required_field(payload, op, key)?;
    let Term::Int(n) = value else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must be an int"
        )));
    };
    let Some(as_i64) = n.to_i64() else {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must fit i64"
        )));
    };
    if as_i64 <= 0 {
        return Err(EffectsError::BadPayload(format!(
            "{op} payload field `{key}` must be > 0"
        )));
    }
    usize::try_from(as_i64).map_err(|_| {
        EffectsError::BadPayload(format!(
            "{op} payload field `{key}` exceeds platform usize range"
        ))
    })
}

fn crypto_positive_usize_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
    key: &str,
) -> Result<usize, String> {
    if let Some(crypto) = pol.and_then(|policy| policy.authorized_crypto.as_ref()) {
        let state = match key {
            "max_aad_bytes" => &crypto.max_aad_bytes,
            "max_ciphertext_bytes" => &crypto.max_ciphertext_bytes,
            "max_context_bytes" => &crypto.max_context_bytes,
            "max_info_bytes" => &crypto.max_info_bytes,
            "max_input_bytes" => &crypto.max_input_bytes,
            "max_message_bytes" => &crypto.max_message_bytes,
            "max_nonce_bytes" => &crypto.max_nonce_bytes,
            "max_output_bytes" => &crypto.max_output_bytes,
            "max_plaintext_bytes" => &crypto.max_plaintext_bytes,
            "max_salt_bytes" => &crypto.max_salt_bytes,
            "max_signature_bytes" => &crypto.max_signature_bytes,
            "max_tag_bytes" => &crypto.max_tag_bytes,
            _ => return Err(format!("unknown crypto policy bound `{key}`")),
        };
        return match state {
            AuthorizedMaxBytes::Absent => {
                Err(format!("{op} requires per-op `{key}` bound in caps.toml"))
            }
            AuthorizedMaxBytes::InvalidType => Err(format!("{key} must be a positive integer")),
            AuthorizedMaxBytes::NonPositive => Err(format!("{key} must be > 0")),
            AuthorizedMaxBytes::PlatformOverflow => Err(format!(
                "{key} is too large for this platform (max {})",
                usize::MAX
            )),
            AuthorizedMaxBytes::Valid(limit) => Ok(*limit),
        };
    }
    match op_extra_positive_usize(pol, key)? {
        Some(value) => Ok(value),
        None => Err(format!("{op} requires per-op `{key}` bound in caps.toml")),
    }
}

fn crypto_allow_algorithms_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Vec<String>, String> {
    if let Some(crypto) = pol.and_then(|policy| policy.authorized_crypto.as_ref()) {
        return authorized_crypto_allowlist(
            &crypto.algorithms,
            "allow_algorithms",
            &format!("{op} requires per-op allow_algorithms allowlist in caps.toml"),
        );
    }
    parse_nonempty_string_array(
        pol,
        "allow_algorithms",
        &format!("{op} requires per-op allow_algorithms allowlist in caps.toml"),
    )
    .map(|values| values.into_iter().map(|x| x.to_ascii_lowercase()).collect())
}

fn crypto_allow_key_ids_from_policy(
    pol: Option<&OpPolicy>,
    op: &str,
) -> Result<Vec<String>, String> {
    if let Some(crypto) = pol.and_then(|policy| policy.authorized_crypto.as_ref()) {
        return authorized_crypto_allowlist(
            &crypto.key_ids,
            "allow_key_ids",
            &format!("{op} requires per-op allow_key_ids allowlist in caps.toml"),
        );
    }
    parse_nonempty_string_array(
        pol,
        "allow_key_ids",
        &format!("{op} requires per-op allow_key_ids allowlist in caps.toml"),
    )
}

fn authorized_crypto_allowlist(
    state: &AuthorizedStringList,
    key: &str,
    missing: &str,
) -> Result<Vec<String>, String> {
    match state {
        AuthorizedStringList::Absent => Err(missing.to_string()),
        AuthorizedStringList::InvalidType => Err(format!("{key} must be an array of strings")),
        AuthorizedStringList::InvalidEntry => Err(format!("{key} entries must be strings")),
        AuthorizedStringList::Empty => Err(format!("{key} must contain at least one entry")),
        AuthorizedStringList::Valid(values) => Ok(values.clone()),
    }
}

fn validate_crypto_algorithm_policy(
    pol: Option<&OpPolicy>,
    op: &str,
    algorithm: &str,
) -> Result<(), String> {
    let allow = crypto_allow_algorithms_from_policy(pol, op)?;
    if allowlist_contains_exact_or_glob_ci(&allow, algorithm) {
        return Ok(());
    }
    Err(format!(
        "algorithm `{algorithm}` is not in allow_algorithms policy"
    ))
}

fn validate_crypto_key_id_policy(
    pol: Option<&OpPolicy>,
    op: &str,
    key_id: &str,
) -> Result<(), String> {
    let allow = crypto_allow_key_ids_from_policy(pol, op)?;
    if allowlist_contains_exact_or_glob(&allow, key_id) {
        return Ok(());
    }
    Err(format!("key id `{key_id}` is not in allow_key_ids policy"))
}

fn check_bound_len(
    error_tok: SealId,
    op: &str,
    subject: &str,
    observed: usize,
    limit: usize,
) -> Option<Value> {
    (observed > limit).then(|| mk_resource_limit_error(error_tok, op, subject, observed, limit))
}

fn crypto_bridge_call(
    bridge_runtime: &mut HostBridgeRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    if !has_explicit_bridge_profile(pol) {
        return Ok(mk_error(
            error_tok,
            "core/caps/backend-unavailable",
            backend_unavailable_message(op),
            Some(op),
        ));
    }
    match call_host_bridge(bridge_runtime, "crypto", op, payload, pol) {
        Ok(resp) => Ok(Value::data(resp)),
        Err(err) => Ok(mk_bridge_error(error_tok, &err, Some(op))),
    }
}

pub(super) fn capability_core_crypto_hash(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let algorithm =
        payload_required_string_or_symbol_field(payload, op, ":algorithm")?.to_ascii_lowercase();
    if let Err(err) = validate_crypto_algorithm_policy(pol, op, &algorithm) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let data = payload_required_bytes_field(payload, op, ":data")?;
    let max_input_bytes = match crypto_positive_usize_from_policy(pol, op, "max_input_bytes") {
        Ok(value) => value,
        Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
    };
    if let Some(limit_err) = check_bound_len(
        error_tok,
        op,
        "crypto input bytes",
        data.len(),
        max_input_bytes,
    ) {
        return Ok(limit_err);
    }
    crypto_bridge_call(bridge_runtime, op, payload, pol, error_tok)
}

pub(super) fn capability_core_crypto_sign(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let algorithm =
        payload_required_string_or_symbol_field(payload, op, ":algorithm")?.to_ascii_lowercase();
    if let Err(err) = validate_crypto_algorithm_policy(pol, op, &algorithm) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let key_id = payload_required_string_field(payload, op, ":key-id")?;
    if let Err(err) = validate_crypto_key_id_policy(pol, op, &key_id) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let message = payload_required_bytes_field(payload, op, ":message")?;
    let max_message_bytes = match crypto_positive_usize_from_policy(pol, op, "max_message_bytes") {
        Ok(value) => value,
        Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
    };
    if let Some(limit_err) = check_bound_len(
        error_tok,
        op,
        "crypto message bytes",
        message.len(),
        max_message_bytes,
    ) {
        return Ok(limit_err);
    }
    if let Some(context) = payload_optional_bytes_field(payload, op, ":context")? {
        let max_context_bytes =
            match crypto_positive_usize_from_policy(pol, op, "max_context_bytes") {
                Ok(value) => value,
                Err(err) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
                }
            };
        if let Some(limit_err) = check_bound_len(
            error_tok,
            op,
            "crypto context bytes",
            context.len(),
            max_context_bytes,
        ) {
            return Ok(limit_err);
        }
    }
    crypto_bridge_call(bridge_runtime, op, payload, pol, error_tok)
}

pub(super) fn capability_core_crypto_verify(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let algorithm =
        payload_required_string_or_symbol_field(payload, op, ":algorithm")?.to_ascii_lowercase();
    if let Err(err) = validate_crypto_algorithm_policy(pol, op, &algorithm) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let key_id = payload_required_string_field(payload, op, ":key-id")?;
    if let Err(err) = validate_crypto_key_id_policy(pol, op, &key_id) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let message = payload_required_bytes_field(payload, op, ":message")?;
    let signature = payload_required_bytes_field(payload, op, ":signature")?;
    let max_message_bytes = match crypto_positive_usize_from_policy(pol, op, "max_message_bytes") {
        Ok(value) => value,
        Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
    };
    let max_signature_bytes =
        match crypto_positive_usize_from_policy(pol, op, "max_signature_bytes") {
            Ok(value) => value,
            Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
        };
    if let Some(limit_err) = check_bound_len(
        error_tok,
        op,
        "crypto message bytes",
        message.len(),
        max_message_bytes,
    ) {
        return Ok(limit_err);
    }
    if let Some(limit_err) = check_bound_len(
        error_tok,
        op,
        "crypto signature bytes",
        signature.len(),
        max_signature_bytes,
    ) {
        return Ok(limit_err);
    }
    if let Some(context) = payload_optional_bytes_field(payload, op, ":context")? {
        let max_context_bytes =
            match crypto_positive_usize_from_policy(pol, op, "max_context_bytes") {
                Ok(value) => value,
                Err(err) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
                }
            };
        if let Some(limit_err) = check_bound_len(
            error_tok,
            op,
            "crypto context bytes",
            context.len(),
            max_context_bytes,
        ) {
            return Ok(limit_err);
        }
    }
    crypto_bridge_call(bridge_runtime, op, payload, pol, error_tok)
}

pub(super) fn capability_core_crypto_kdf(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let algorithm =
        payload_required_string_or_symbol_field(payload, op, ":algorithm")?.to_ascii_lowercase();
    if let Err(err) = validate_crypto_algorithm_policy(pol, op, &algorithm) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let key_id = payload_required_string_field(payload, op, ":key-id")?;
    if let Err(err) = validate_crypto_key_id_policy(pol, op, &key_id) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let info = payload_required_bytes_field(payload, op, ":info")?;
    let length = payload_required_positive_usize_field(payload, op, ":length")?;
    let max_info_bytes = match crypto_positive_usize_from_policy(pol, op, "max_info_bytes") {
        Ok(value) => value,
        Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
    };
    let max_output_bytes = match crypto_positive_usize_from_policy(pol, op, "max_output_bytes") {
        Ok(value) => value,
        Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
    };
    if let Some(limit_err) = check_bound_len(
        error_tok,
        op,
        "crypto info bytes",
        info.len(),
        max_info_bytes,
    ) {
        return Ok(limit_err);
    }
    if let Some(limit_err) = check_bound_len(
        error_tok,
        op,
        "crypto output bytes",
        length,
        max_output_bytes,
    ) {
        return Ok(limit_err);
    }
    if let Some(salt) = payload_optional_bytes_field(payload, op, ":salt")? {
        let max_salt_bytes = match crypto_positive_usize_from_policy(pol, op, "max_salt_bytes") {
            Ok(value) => value,
            Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
        };
        if let Some(limit_err) = check_bound_len(
            error_tok,
            op,
            "crypto salt bytes",
            salt.len(),
            max_salt_bytes,
        ) {
            return Ok(limit_err);
        }
    }
    crypto_bridge_call(bridge_runtime, op, payload, pol, error_tok)
}

pub(super) fn capability_core_crypto_aead_seal(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let algorithm =
        payload_required_string_or_symbol_field(payload, op, ":algorithm")?.to_ascii_lowercase();
    if let Err(err) = validate_crypto_algorithm_policy(pol, op, &algorithm) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let key_id = payload_required_string_field(payload, op, ":key-id")?;
    if let Err(err) = validate_crypto_key_id_policy(pol, op, &key_id) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let plaintext = payload_required_bytes_field(payload, op, ":plaintext")?;
    let max_plaintext_bytes =
        match crypto_positive_usize_from_policy(pol, op, "max_plaintext_bytes") {
            Ok(value) => value,
            Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
        };
    if let Some(limit_err) = check_bound_len(
        error_tok,
        op,
        "crypto plaintext bytes",
        plaintext.len(),
        max_plaintext_bytes,
    ) {
        return Ok(limit_err);
    }
    if let Some(aad) = payload_optional_bytes_field(payload, op, ":aad")? {
        let max_aad_bytes = match crypto_positive_usize_from_policy(pol, op, "max_aad_bytes") {
            Ok(value) => value,
            Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
        };
        if let Some(limit_err) =
            check_bound_len(error_tok, op, "crypto aad bytes", aad.len(), max_aad_bytes)
        {
            return Ok(limit_err);
        }
    }
    if let Some(nonce) = payload_optional_bytes_field(payload, op, ":nonce")? {
        let max_nonce_bytes = match crypto_positive_usize_from_policy(pol, op, "max_nonce_bytes") {
            Ok(value) => value,
            Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
        };
        if let Some(limit_err) = check_bound_len(
            error_tok,
            op,
            "crypto nonce bytes",
            nonce.len(),
            max_nonce_bytes,
        ) {
            return Ok(limit_err);
        }
    }
    crypto_bridge_call(bridge_runtime, op, payload, pol, error_tok)
}

pub(super) fn capability_core_crypto_aead_open(
    op: &str,
    bridge_runtime: &mut HostBridgeRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let algorithm =
        payload_required_string_or_symbol_field(payload, op, ":algorithm")?.to_ascii_lowercase();
    if let Err(err) = validate_crypto_algorithm_policy(pol, op, &algorithm) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let key_id = payload_required_string_field(payload, op, ":key-id")?;
    if let Err(err) = validate_crypto_key_id_policy(pol, op, &key_id) {
        return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op)));
    }
    let ciphertext = payload_required_bytes_field(payload, op, ":ciphertext")?;
    let max_ciphertext_bytes =
        match crypto_positive_usize_from_policy(pol, op, "max_ciphertext_bytes") {
            Ok(value) => value,
            Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
        };
    if let Some(limit_err) = check_bound_len(
        error_tok,
        op,
        "crypto ciphertext bytes",
        ciphertext.len(),
        max_ciphertext_bytes,
    ) {
        return Ok(limit_err);
    }
    if let Some(aad) = payload_optional_bytes_field(payload, op, ":aad")? {
        let max_aad_bytes = match crypto_positive_usize_from_policy(pol, op, "max_aad_bytes") {
            Ok(value) => value,
            Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
        };
        if let Some(limit_err) =
            check_bound_len(error_tok, op, "crypto aad bytes", aad.len(), max_aad_bytes)
        {
            return Ok(limit_err);
        }
    }
    if let Some(nonce) = payload_optional_bytes_field(payload, op, ":nonce")? {
        let max_nonce_bytes = match crypto_positive_usize_from_policy(pol, op, "max_nonce_bytes") {
            Ok(value) => value,
            Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
        };
        if let Some(limit_err) = check_bound_len(
            error_tok,
            op,
            "crypto nonce bytes",
            nonce.len(),
            max_nonce_bytes,
        ) {
            return Ok(limit_err);
        }
    }
    if let Some(tag) = payload_optional_bytes_field(payload, op, ":tag")? {
        let max_tag_bytes = match crypto_positive_usize_from_policy(pol, op, "max_tag_bytes") {
            Ok(value) => value,
            Err(err) => return Ok(mk_error(error_tok, "core/caps/policy-error", err, Some(op))),
        };
        if let Some(limit_err) =
            check_bound_len(error_tok, op, "crypto tag bytes", tag.len(), max_tag_bytes)
        {
            return Ok(limit_err);
        }
    }
    crypto_bridge_call(bridge_runtime, op, payload, pol, error_tok)
}

#[cfg(test)]
mod authority_tests {
    use std::collections::BTreeMap;

    use toml::Value as TomlValue;

    use super::*;

    fn absent_crypto() -> AuthorizedCryptoPolicy {
        AuthorizedCryptoPolicy {
            algorithms: AuthorizedStringList::Absent,
            key_ids: AuthorizedStringList::Absent,
            max_aad_bytes: AuthorizedMaxBytes::Absent,
            max_ciphertext_bytes: AuthorizedMaxBytes::Absent,
            max_context_bytes: AuthorizedMaxBytes::Absent,
            max_info_bytes: AuthorizedMaxBytes::Absent,
            max_input_bytes: AuthorizedMaxBytes::Absent,
            max_message_bytes: AuthorizedMaxBytes::Absent,
            max_nonce_bytes: AuthorizedMaxBytes::Absent,
            max_output_bytes: AuthorizedMaxBytes::Absent,
            max_plaintext_bytes: AuthorizedMaxBytes::Absent,
            max_salt_bytes: AuthorizedMaxBytes::Absent,
            max_signature_bytes: AuthorizedMaxBytes::Absent,
            max_tag_bytes: AuthorizedMaxBytes::Absent,
        }
    }

    fn policy(crypto: AuthorizedCryptoPolicy) -> OpPolicy {
        OpPolicy {
            base_dir: None,
            create_dirs: false,
            timeout_ms: None,
            log_inline_max_bytes: None,
            extra: BTreeMap::from([
                (
                    "allow_algorithms".to_string(),
                    TomlValue::String("raw fallback must not be used".to_string()),
                ),
                (
                    "allow_key_ids".to_string(),
                    TomlValue::String("raw fallback must not be used".to_string()),
                ),
                (
                    "max_message_bytes".to_string(),
                    TomlValue::String("raw fallback must not be used".to_string()),
                ),
            ]),
            authorized_cap: None,
            authorized_max_bytes: None,
            authorized_process_programs: None,
            authorized_database: None,
            authorized_network: None,
            authorized_crypto: Some(crypto),
            authorized_gpu: None,
            authorized_gfx_profile: None,
            authorized_bridge_identity: None,
            authorized_plugin: None,
            authorized_ffi: None,
        }
    }

    #[test]
    fn crypto_dispatch_consumes_authorized_policy_before_raw_policy() {
        let mut crypto = absent_crypto();
        crypto.algorithms = AuthorizedStringList::Valid(vec!["ed25519".to_string()]);
        crypto.key_ids = AuthorizedStringList::Valid(vec!["key-main".to_string()]);
        crypto.max_message_bytes = AuthorizedMaxBytes::Valid(4096);
        let policy = policy(crypto);
        assert_eq!(
            crypto_allow_algorithms_from_policy(Some(&policy), "core/crypto::sign").unwrap(),
            vec!["ed25519"]
        );
        assert_eq!(
            crypto_allow_key_ids_from_policy(Some(&policy), "core/crypto::sign").unwrap(),
            vec!["key-main"]
        );
        assert_eq!(
            crypto_positive_usize_from_policy(
                Some(&policy),
                "core/crypto::sign",
                "max_message_bytes"
            )
            .unwrap(),
            4096
        );
    }

    #[test]
    fn crypto_dispatch_preserves_authorized_policy_errors() {
        let mut crypto = absent_crypto();
        crypto.algorithms = AuthorizedStringList::InvalidEntry;
        crypto.max_message_bytes = AuthorizedMaxBytes::NonPositive;
        let policy = policy(crypto);
        assert_eq!(
            crypto_allow_algorithms_from_policy(Some(&policy), "core/crypto::sign").unwrap_err(),
            "allow_algorithms entries must be strings"
        );
        assert_eq!(
            crypto_positive_usize_from_policy(
                Some(&policy),
                "core/crypto::sign",
                "max_message_bytes"
            )
            .unwrap_err(),
            "max_message_bytes must be > 0"
        );
    }
}
