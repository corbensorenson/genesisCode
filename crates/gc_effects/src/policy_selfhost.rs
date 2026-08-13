use std::path::{Path, PathBuf};

use gc_prelude::SelfhostBootstrapMode;

use super::{CapsPolicy, EffectsError, policy_authority, policy_transport};

#[derive(Debug, Clone)]
pub(crate) struct SelfhostAuthorityConfig {
    pub(crate) bootstrap_mode: SelfhostBootstrapMode,
    pub(crate) artifact: Option<PathBuf>,
}

impl CapsPolicy {
    pub fn from_toml_str_with_selfhost_authority(
        source: &str,
        bootstrap_mode: SelfhostBootstrapMode,
        artifact: Option<&Path>,
    ) -> Result<Self, EffectsError> {
        let mut policy = policy_transport::decode_selfhost_transport(source)?;
        policy_authority::authorize_policy(source, &mut policy, bootstrap_mode, artifact)?;
        policy.selfhost_authority = Some(SelfhostAuthorityConfig {
            bootstrap_mode,
            artifact: artifact.map(Path::to_path_buf),
        });
        Ok(policy)
    }

    pub(crate) fn selfhost_authority_config(&self) -> Option<&SelfhostAuthorityConfig> {
        self.selfhost_authority.as_ref()
    }
}
