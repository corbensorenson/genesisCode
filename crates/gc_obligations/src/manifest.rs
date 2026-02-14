use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ObligationError;

#[derive(Debug, Clone, Deserialize)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub modules: Vec<ModuleEntry>,

    #[serde(default)]
    pub dependencies: Vec<DepEntry>,

    pub obligations: Vec<String>,

    #[serde(default)]
    pub tests: Vec<String>,

    pub caps_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModuleEntry {
    pub path: String,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DepEntry {
    pub name: String,
    pub path: String,
    pub hash: Option<String>,
}

impl PackageManifest {
    pub fn load(path: &Path) -> Result<(Self, PathBuf), ObligationError> {
        let s = std::fs::read_to_string(path)?;
        let m: PackageManifest = toml::from_str(&s).map_err(|e| {
            ObligationError::Manifest(format!("{}: {e}", path.display()))
        })?;
        let dir = path
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| ObligationError::Manifest("package.toml has no parent dir".to_string()))?;
        Ok((m, dir))
    }
}

