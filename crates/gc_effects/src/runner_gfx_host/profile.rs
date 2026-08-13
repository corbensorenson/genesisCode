use crate::policy::{AuthorizedGfxProfile, OpPolicy};

pub(super) type GfxFirstPartyProfile = AuthorizedGfxProfile;

pub(super) fn first_party_profile(
    policy: Option<&OpPolicy>,
) -> Result<GfxFirstPartyProfile, String> {
    let Some(policy) = policy else {
        return Ok(GfxFirstPartyProfile::Headless);
    };
    policy
        .authorized_gfx_profile
        .ok_or_else(|| "missing GenesisCode GFX profile authority".to_string())
}

pub(super) fn profile_label(profile: GfxFirstPartyProfile) -> &'static str {
    match profile {
        GfxFirstPartyProfile::Headless => "headless",
        GfxFirstPartyProfile::Interactive => "interactive",
        GfxFirstPartyProfile::Desktop => "desktop",
        GfxFirstPartyProfile::Browser => "browser",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use toml::Value as TomlValue;

    use super::*;

    fn policy(
        entries: &[(&str, TomlValue)],
        authorized_gfx_profile: Option<AuthorizedGfxProfile>,
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
            authorized_gfx_profile,
            authorized_xr_policy: None,
            authorized_bridge_identity: None,
            authorized_plugin: None,
            authorized_ffi: None,
        }
    }

    #[test]
    fn gfx_profile_consumes_authority_before_raw_policy() {
        let policy = policy(
            &[(
                "first_party_profile",
                TomlValue::String("headless".to_string()),
            )],
            Some(AuthorizedGfxProfile::Browser),
        );
        assert_eq!(
            first_party_profile(Some(&policy)).unwrap(),
            AuthorizedGfxProfile::Browser
        );
    }

    #[test]
    fn absent_policy_defaults_headless_but_missing_authority_fails_closed() {
        assert_eq!(
            first_party_profile(None).unwrap(),
            AuthorizedGfxProfile::Headless
        );
        let policy = policy(&[], None);
        assert!(first_party_profile(Some(&policy)).is_err());
    }
}
