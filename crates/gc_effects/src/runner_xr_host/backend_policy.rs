use crate::policy::{AuthorizedXrPolicy, OpPolicy};

pub(super) fn authorized_policy(policy: Option<&OpPolicy>) -> Result<AuthorizedXrPolicy, String> {
    let Some(policy) = policy else {
        return Ok(AuthorizedXrPolicy::default());
    };
    policy
        .authorized_xr_policy
        .clone()
        .ok_or_else(|| "missing GenesisCode XR policy authority".to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use toml::Value as TomlValue;

    use super::*;
    use crate::policy::AuthorizedXrBackend;

    fn policy(
        entries: &[(&str, TomlValue)],
        authorized_xr_policy: Option<AuthorizedXrPolicy>,
    ) -> OpPolicy {
        OpPolicy {
            base_dir: None,
            create_dirs: false,
            timeout_ms: None,
            log_inline_max_bytes: None,
            extra: entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
            authorized_cap: None,
            authorized_max_bytes: None,
            authorized_process_programs: None,
            authorized_database: None,
            authorized_network: None,
            authorized_crypto: None,
            authorized_gpu: None,
            authorized_gfx_profile: None,
            authorized_xr_policy,
            authorized_bridge_identity: None,
            authorized_plugin: None,
            authorized_ffi: None,
        }
    }

    #[test]
    fn xr_consumes_authority_before_raw_policy() {
        let authorized = AuthorizedXrPolicy {
            backend: AuthorizedXrBackend::WebxrDevice,
            ..AuthorizedXrPolicy::default()
        };
        let policy = policy(
            &[(
                "xr_backend",
                TomlValue::String("first-party-runtime".to_string()),
            )],
            Some(authorized.clone()),
        );
        assert_eq!(authorized_policy(Some(&policy)).unwrap(), authorized);
    }

    #[test]
    fn absent_policy_defaults_but_present_policy_without_authority_fails_closed() {
        assert_eq!(
            authorized_policy(None).unwrap(),
            AuthorizedXrPolicy::default()
        );
        assert!(authorized_policy(Some(&policy(&[], None))).is_err());
    }
}
