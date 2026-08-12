use super::*;
use crate::policy::{AuthorizedMaxBytes, AuthorizedOptionalString, AuthorizedStringList};

pub(super) fn signed_policy_required(pol: Option<&OpPolicy>) -> bool {
    if let Some(ffi) = pol.and_then(|policy| policy.authorized_ffi.as_ref()) {
        return ffi.signed_policy_required;
    }
    pol.and_then(|policy| policy.extra.get("signed_policy_required"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn required_signed_string(
    pol: Option<&OpPolicy>,
    key: &str,
    op: &str,
) -> Result<String, String> {
    if let Some(ffi) = pol.and_then(|policy| policy.authorized_ffi.as_ref()) {
        let state = match key {
            "policy_artifact_h" => &ffi.policy_artifact_h,
            "policy_signature_h" => &ffi.policy_signature_h,
            "policy_key_id" => &ffi.policy_key_id,
            "evidence_mode" => &ffi.evidence_mode,
            _ => return Err(format!("unknown ffi signed-policy field `{key}`")),
        };
        return match state {
            AuthorizedOptionalString::Absent | AuthorizedOptionalString::InvalidType => Err(
                format!("{op} requires per-op {key} when signed_policy_required=true"),
            ),
            AuthorizedOptionalString::Empty => Err(format!(
                "{op} requires non-empty {key} when signed_policy_required=true"
            )),
            AuthorizedOptionalString::Valid(value) => Ok(value.clone()),
        };
    }
    let Some(raw) = pol
        .and_then(|policy| policy.extra.get(key))
        .and_then(toml::Value::as_str)
    else {
        return Err(format!(
            "{op} requires per-op {key} when signed_policy_required=true"
        ));
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!(
            "{op} requires non-empty {key} when signed_policy_required=true"
        ));
    }
    Ok(trimmed.to_string())
}

fn authorized_allowlist(
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

pub(super) fn allowlist_from_policy(
    pol: Option<&OpPolicy>,
    key: &str,
    op: &str,
) -> Result<Vec<String>, String> {
    let missing = format!("{op} requires per-op {key} allowlist in caps.toml");
    if let Some(ffi) = pol.and_then(|policy| policy.authorized_ffi.as_ref()) {
        let state = match key {
            "allow_abi_ids" => &ffi.abi_ids,
            "allow_libraries" => &ffi.libraries,
            "allow_symbols" => &ffi.symbols,
            _ => return Err(format!("unknown ffi policy allowlist `{key}`")),
        };
        return authorized_allowlist(state, key, &missing);
    }
    parse_nonempty_string_array(pol, key, &missing)
}

pub(super) fn schema_allowlist_from_policy(
    pol: Option<&OpPolicy>,
) -> Result<Option<Vec<String>>, String> {
    if let Some(ffi) = pol.and_then(|policy| policy.authorized_ffi.as_ref()) {
        return match &ffi.schema_ids {
            AuthorizedStringList::Absent => Ok(None),
            state => authorized_allowlist(
                state,
                "allow_schema_ids",
                "allow_schema_ids must be configured with at least one entry",
            )
            .map(Some),
        };
    }
    let Some(pol) = pol else {
        return Ok(None);
    };
    if !pol.extra.contains_key("allow_schema_ids") {
        return Ok(None);
    }
    parse_nonempty_string_array(
        Some(pol),
        "allow_schema_ids",
        "allow_schema_ids must be configured with at least one entry",
    )
    .map(Some)
}

pub(super) fn positive_usize_from_policy(
    pol: Option<&OpPolicy>,
    key: &str,
) -> Result<Option<usize>, String> {
    if let Some(ffi) = pol.and_then(|policy| policy.authorized_ffi.as_ref()) {
        let state = match key {
            "max_buffer_bytes" => &ffi.max_buffer_bytes,
            "max_call_payload_bytes" => &ffi.max_call_payload_bytes,
            _ => return Err(format!("unknown ffi policy bound `{key}`")),
        };
        return match state {
            AuthorizedMaxBytes::Absent => Ok(None),
            AuthorizedMaxBytes::InvalidType => Err(format!("{key} must be a positive integer")),
            AuthorizedMaxBytes::NonPositive => Err(format!("{key} must be > 0")),
            AuthorizedMaxBytes::PlatformOverflow => Err(format!(
                "{key} is too large for this platform (max {})",
                usize::MAX
            )),
            AuthorizedMaxBytes::Valid(limit) => Ok(Some(*limit)),
        };
    }
    op_extra_positive_usize(pol, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::AuthorizedFfiPolicy;
    use toml::Value as TomlValue;

    fn policy(ffi: AuthorizedFfiPolicy) -> OpPolicy {
        OpPolicy {
            base_dir: None,
            create_dirs: false,
            timeout_ms: None,
            log_inline_max_bytes: None,
            extra: BTreeMap::from([
                (
                    "allow_abi_ids".to_string(),
                    TomlValue::String("raw fallback must not be used".to_string()),
                ),
                (
                    "max_buffer_bytes".to_string(),
                    TomlValue::String("raw fallback must not be used".to_string()),
                ),
            ]),
            authorized_cap: None,
            authorized_max_bytes: None,
            authorized_process_programs: None,
            authorized_database: None,
            authorized_network: None,
            authorized_crypto: None,
            authorized_plugin: None,
            authorized_ffi: Some(ffi),
        }
    }

    #[test]
    fn ffi_dispatch_consumes_authorized_policy_before_raw_policy() {
        let policy = policy(AuthorizedFfiPolicy {
            abi_ids: AuthorizedStringList::Valid(vec!["abi.v1".to_string()]),
            libraries: AuthorizedStringList::Valid(vec!["libmath.so".to_string()]),
            symbols: AuthorizedStringList::Valid(vec!["sum".to_string()]),
            schema_ids: AuthorizedStringList::Valid(vec!["schema.v1".to_string()]),
            max_buffer_bytes: AuthorizedMaxBytes::Valid(64),
            max_call_payload_bytes: AuthorizedMaxBytes::Valid(128),
            signed_policy_required: true,
            policy_artifact_h: AuthorizedOptionalString::Valid("aa".repeat(32)),
            policy_signature_h: AuthorizedOptionalString::Valid("bb".repeat(32)),
            policy_key_id: AuthorizedOptionalString::Valid("root-key".to_string()),
            evidence_mode: AuthorizedOptionalString::Valid("deterministic".to_string()),
        });
        assert_eq!(
            allowlist_from_policy(Some(&policy), "allow_abi_ids", "host/ffi::call").unwrap(),
            vec!["abi.v1"]
        );
        assert_eq!(
            allowlist_from_policy(Some(&policy), "allow_libraries", "host/ffi::call").unwrap(),
            vec!["libmath.so"]
        );
        assert_eq!(
            allowlist_from_policy(Some(&policy), "allow_symbols", "host/ffi::call").unwrap(),
            vec!["sum"]
        );
        assert_eq!(
            schema_allowlist_from_policy(Some(&policy)).unwrap(),
            Some(vec!["schema.v1".to_string()])
        );
        assert_eq!(
            positive_usize_from_policy(Some(&policy), "max_buffer_bytes").unwrap(),
            Some(64)
        );
        assert_eq!(
            positive_usize_from_policy(Some(&policy), "max_call_payload_bytes").unwrap(),
            Some(128)
        );
        assert!(signed_policy_required(Some(&policy)));
        assert_eq!(
            required_signed_string(Some(&policy), "policy_key_id", "host/ffi::call").unwrap(),
            "root-key"
        );
    }

    #[test]
    fn ffi_dispatch_preserves_authorized_policy_errors_and_optional_schema() {
        let policy = policy(AuthorizedFfiPolicy {
            abi_ids: AuthorizedStringList::InvalidEntry,
            libraries: AuthorizedStringList::Empty,
            symbols: AuthorizedStringList::InvalidType,
            schema_ids: AuthorizedStringList::Absent,
            max_buffer_bytes: AuthorizedMaxBytes::NonPositive,
            max_call_payload_bytes: AuthorizedMaxBytes::PlatformOverflow,
            signed_policy_required: true,
            policy_artifact_h: AuthorizedOptionalString::InvalidType,
            policy_signature_h: AuthorizedOptionalString::Empty,
            policy_key_id: AuthorizedOptionalString::Absent,
            evidence_mode: AuthorizedOptionalString::Valid("random".to_string()),
        });
        assert_eq!(
            allowlist_from_policy(Some(&policy), "allow_abi_ids", "host/ffi::call").unwrap_err(),
            "allow_abi_ids entries must be strings"
        );
        assert_eq!(
            allowlist_from_policy(Some(&policy), "allow_libraries", "host/ffi::call").unwrap_err(),
            "allow_libraries must contain at least one entry"
        );
        assert_eq!(
            allowlist_from_policy(Some(&policy), "allow_symbols", "host/ffi::call").unwrap_err(),
            "allow_symbols must be an array of strings"
        );
        assert_eq!(schema_allowlist_from_policy(Some(&policy)).unwrap(), None);
        assert_eq!(
            positive_usize_from_policy(Some(&policy), "max_buffer_bytes").unwrap_err(),
            "max_buffer_bytes must be > 0"
        );
        assert_eq!(
            positive_usize_from_policy(Some(&policy), "max_call_payload_bytes").unwrap_err(),
            format!(
                "max_call_payload_bytes is too large for this platform (max {})",
                usize::MAX
            )
        );
        assert_eq!(
            required_signed_string(Some(&policy), "policy_artifact_h", "host/ffi::call")
                .unwrap_err(),
            "host/ffi::call requires per-op policy_artifact_h when signed_policy_required=true"
        );
        assert_eq!(
            required_signed_string(Some(&policy), "policy_signature_h", "host/ffi::call")
                .unwrap_err(),
            "host/ffi::call requires non-empty policy_signature_h when signed_policy_required=true"
        );
    }
}
