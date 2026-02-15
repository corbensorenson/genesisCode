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

    #[serde(default)]
    pub property_tests: Vec<String>,

    pub caps_policy: Option<String>,

    #[serde(default)]
    pub limits: Limits,

    #[serde(default)]
    pub budgets: Budgets,

    #[serde(default)]
    pub property: PropertyConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Limits {
    /// Kernel evaluation step limit for package evaluation and tests.
    ///
    /// If omitted, the toolchain default is used.
    pub step_limit: Option<u64>,

    /// Allow disabling the step limit via CLI (`--no-step-limit`).
    ///
    /// Default is deny (false).
    #[serde(default)]
    pub allow_unlimited: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Budgets {
    /// If set, each unit test must complete within this many kernel evaluation steps.
    pub max_steps_per_test: Option<u64>,

    /// If set, each effectful test must produce no more than this many effect log entries.
    pub max_effect_entries_per_test: Option<u64>,

    /// If set, each effect log must serialize to at most this many bytes in canonical CoreForm.
    pub max_effect_log_bytes_per_test: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PropertyConfig {
    /// Default number of cases per property test, if the test entry does not specify `:cases`.
    pub cases_per_test: Option<u64>,
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
        let m: PackageManifest = toml::from_str(&s)
            .map_err(|e| ObligationError::Manifest(format!("{}: {e}", path.display())))?;
        validate_manifest_paths(path, &m)?;
        let dir = path.parent().map(PathBuf::from).ok_or_else(|| {
            ObligationError::Manifest("package.toml has no parent dir".to_string())
        })?;
        Ok((m, dir))
    }
}

fn validate_manifest_paths(pkg_toml: &Path, m: &PackageManifest) -> Result<(), ObligationError> {
    for (i, me) in m.modules.iter().enumerate() {
        validate_rel_path_str(
            &me.path,
            &format!("modules[{i}].path"),
            pkg_toml,
        )?;
    }
    for (i, de) in m.dependencies.iter().enumerate() {
        validate_rel_path_str(
            &de.path,
            &format!("dependencies[{i}].path"),
            pkg_toml,
        )?;
    }
    if let Some(p) = &m.caps_policy {
        validate_rel_path_str(p, "caps_policy", pkg_toml)?;
    }
    Ok(())
}

fn validate_rel_path_str(s: &str, field: &str, pkg_toml: &Path) -> Result<(), ObligationError> {
    if s.is_empty() {
        return Err(ObligationError::Manifest(format!(
            "{}: {field} must be non-empty",
            pkg_toml.display()
        )));
    }
    if s.contains('\\') {
        return Err(ObligationError::Manifest(format!(
            "{}: {field} must use '/' path separators (got backslash)",
            pkg_toml.display()
        )));
    }
    let p = Path::new(s);
    if p.is_absolute() {
        return Err(ObligationError::Manifest(format!(
            "{}: {field} must be a relative path, got {s}",
            pkg_toml.display()
        )));
    }
    for c in p.components() {
        match c {
            std::path::Component::Normal(_) => {}
            // Disallow '.', '..', Windows prefixes, and root dirs to avoid non-portable and unsafe paths.
            _ => {
                return Err(ObligationError::Manifest(format!(
                    "{}: {field} must not contain '.', '..', or absolute/prefix components: {s}",
                    pkg_toml.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PackageManifest;

    #[test]
    fn rejects_parent_dir_in_module_path() {
        let td = tempfile::tempdir().unwrap();
        let pkg = td.path().join("package.toml");
        std::fs::write(
            &pkg,
            r#"
name = "x"
version = "0.0.1"
obligations = []
dependencies = []

[[modules]]
path = "../escape.gc"
hash = ""
"#,
        )
        .unwrap();

        let e = PackageManifest::load(&pkg).unwrap_err();
        let s = format!("{e}");
        assert!(s.contains("must not contain"), "{s}");
    }
}
