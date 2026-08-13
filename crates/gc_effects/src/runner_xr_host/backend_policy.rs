use crate::policy::{AuthorizedXrBackend, OpPolicy};

pub(super) fn authorized_backend(policy: Option<&OpPolicy>) -> Result<AuthorizedXrBackend, String> {
    let Some(policy) = policy else {
        return Ok(AuthorizedXrBackend::FirstParty);
    };
    policy
        .authorized_xr_backend
        .clone()
        .ok_or_else(|| "missing GenesisCode XR backend authority".to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use toml::Value as TomlValue;

    use super::*;

    fn policy(
        entries: &[(&str, TomlValue)],
        authorized_xr_backend: Option<AuthorizedXrBackend>,
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
            authorized_xr_backend,
            authorized_bridge_identity: None,
            authorized_plugin: None,
            authorized_ffi: None,
        }
    }

    #[test]
    fn xr_backend_consumes_authority_before_raw_policy() {
        let policy = policy(
            &[(
                "xr_backend",
                TomlValue::String("first-party-runtime".to_string()),
            )],
            Some(AuthorizedXrBackend::WebxrDevice),
        );
        assert_eq!(
            authorized_backend(Some(&policy)).unwrap(),
            AuthorizedXrBackend::WebxrDevice
        );
    }

    #[test]
    fn absent_policy_defaults_first_party_but_missing_authority_fails_closed() {
        assert_eq!(
            authorized_backend(None).unwrap(),
            AuthorizedXrBackend::FirstParty
        );
        assert!(authorized_backend(Some(&policy(&[], None))).is_err());
    }
}
