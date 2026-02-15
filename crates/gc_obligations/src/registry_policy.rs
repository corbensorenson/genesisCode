use base64ct::{Base64, Encoding};
use ed25519_dalek::VerifyingKey;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegistryPolicyError {
    #[error("policy parse error: {0}")]
    Parse(String),
    #[error("policy invalid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryPolicy {
    pub version: u64,

    /// Minimum number of valid signatures required for the acceptance artifact.
    #[serde(default)]
    pub min_signatures: u64,

    /// Allowed Ed25519 public keys (base64-encoded 32 bytes).
    #[serde(default)]
    pub allowed_public_keys: Vec<String>,
}

impl RegistryPolicy {
    pub fn load(path: &std::path::Path) -> Result<Self, RegistryPolicyError> {
        let s = std::fs::read_to_string(path)
            .map_err(|e| RegistryPolicyError::Parse(format!("{}: {e}", path.display())))?;
        let p: RegistryPolicy =
            toml::from_str(&s).map_err(|e| RegistryPolicyError::Parse(format!("{e}")))?;
        if p.version != 1 {
            return Err(RegistryPolicyError::Invalid(format!(
                "unsupported version {}",
                p.version
            )));
        }
        if p.min_signatures > 0 && p.allowed_public_keys.is_empty() {
            return Err(RegistryPolicyError::Invalid(
                "min_signatures > 0 but allowed_public_keys is empty".to_string(),
            ));
        }
        // Validate keys early.
        for k in &p.allowed_public_keys {
            let _ = decode_pk(k).map_err(RegistryPolicyError::Invalid)?;
        }
        Ok(p)
    }

    pub fn allowed_verifying_keys(&self) -> Result<Vec<VerifyingKey>, RegistryPolicyError> {
        let mut out = Vec::new();
        for k in &self.allowed_public_keys {
            let pk = decode_pk(k).map_err(RegistryPolicyError::Invalid)?;
            let vk = VerifyingKey::from_bytes(&pk)
                .map_err(|e| RegistryPolicyError::Invalid(format!("{e}")))?;
            out.push(vk);
        }
        Ok(out)
    }
}

fn decode_pk(s: &str) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    Base64::decode(s, &mut out).map_err(|e| format!("invalid base64 pk: {e}"))?;
    Ok(out)
}
