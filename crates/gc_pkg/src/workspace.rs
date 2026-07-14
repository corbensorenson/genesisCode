use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

pub const GENESIS_WORKSPACE_VERSION: u64 = 1;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("workspace parse error: {path}: {msg}")]
    Parse { path: String, msg: String },

    #[error("workspace invalid: {path}: {msg}")]
    Invalid { path: String, msg: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMember {
    pub name: String,
    pub path: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceTask {
    pub cmd: String,
    pub file: Option<String>,
    pub pkg: Option<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceProfile {
    pub caps_policy: Option<String>,
    pub registry: Option<String>,
    pub policy: Option<String>,
    pub toolchain: Option<String>,
    pub runtime_backend: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDefaults {
    pub registry: Option<String>,
    pub policy: Option<String>,
    pub toolchain: Option<String>,
    pub runtime_backend: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceConfig {
    pub version: u64,
    pub workspace: String,
    pub members: Vec<WorkspaceMember>,
    pub defaults: WorkspaceDefaults,
    pub profiles: BTreeMap<String, WorkspaceProfile>,
    pub tasks: BTreeMap<String, WorkspaceTask>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceToml {
    version: Option<u64>,
    workspace: Option<String>,
    #[serde(default)]
    members: Vec<MemberToml>,
    #[serde(default)]
    defaults: DefaultsToml,
    #[serde(default)]
    profiles: BTreeMap<String, ProfileToml>,
    #[serde(default)]
    tasks: BTreeMap<String, TaskToml>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct DefaultsToml {
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    policy: Option<String>,
    #[serde(default)]
    toolchain: Option<String>,
    #[serde(default)]
    runtime_backend: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MemberToml {
    name: String,
    path: String,
    #[serde(default)]
    role: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ProfileToml {
    #[serde(default)]
    caps_policy: Option<String>,
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    policy: Option<String>,
    #[serde(default)]
    toolchain: Option<String>,
    #[serde(default)]
    runtime_backend: Option<String>,
}

pub const RUNTIME_BACKEND_HEADLESS: &str = "headless";
pub const RUNTIME_BACKEND_GPU: &str = "gpu";
pub const RUNTIME_BACKEND_GFX: &str = "gfx";
pub const RUNTIME_BACKEND_BACKEND: &str = "backend";

pub fn normalize_runtime_backend_profile(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "headless" | "profile-headless" => Some(RUNTIME_BACKEND_HEADLESS.to_string()),
        "gpu" | "profile-gpu" => Some(RUNTIME_BACKEND_GPU.to_string()),
        "gfx" | "profile-gfx" => Some(RUNTIME_BACKEND_GFX.to_string()),
        "backend" | "profile-backend" => Some(RUNTIME_BACKEND_BACKEND.to_string()),
        _ => None,
    }
}

pub fn runtime_backend_profile_is_compatible(selected: &str, active: &str) -> bool {
    let Some(selected_norm) = normalize_runtime_backend_profile(selected) else {
        return false;
    };
    let Some(active_norm) = normalize_runtime_backend_profile(active) else {
        return false;
    };

    match selected_norm.as_str() {
        RUNTIME_BACKEND_HEADLESS => true,
        RUNTIME_BACKEND_GPU => {
            active_norm == RUNTIME_BACKEND_GPU || active_norm == RUNTIME_BACKEND_BACKEND
        }
        RUNTIME_BACKEND_GFX => {
            active_norm == RUNTIME_BACKEND_GFX || active_norm == RUNTIME_BACKEND_BACKEND
        }
        RUNTIME_BACKEND_BACKEND => active_norm == RUNTIME_BACKEND_BACKEND,
        _ => false,
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TaskToml {
    cmd: String,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    pkg: Option<String>,
    #[serde(default)]
    args: Vec<String>,
}

impl WorkspaceConfig {
    pub fn empty(workspace: impl Into<String>) -> Self {
        let workspace = workspace.into();
        Self {
            version: GENESIS_WORKSPACE_VERSION,
            workspace: workspace.clone(),
            members: vec![WorkspaceMember {
                name: workspace,
                path: ".".to_string(),
                role: Some("root".to_string()),
            }],
            defaults: WorkspaceDefaults {
                registry: None,
                policy: Some("policy:default-v0.1".to_string()),
                toolchain: None,
                runtime_backend: None,
            },
            profiles: BTreeMap::new(),
            tasks: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, WorkspaceError> {
        let s = std::fs::read_to_string(path)?;
        Self::from_toml_str(path, &s)
    }

    pub fn from_toml_str(path: &Path, s: &str) -> Result<Self, WorkspaceError> {
        let wt: WorkspaceToml = toml::from_str(s).map_err(|e| WorkspaceError::Parse {
            path: path.display().to_string(),
            msg: e.to_string(),
        })?;
        let version = wt.version.ok_or_else(|| WorkspaceError::Invalid {
            path: path.display().to_string(),
            msg: "missing version".to_string(),
        })?;
        if version != GENESIS_WORKSPACE_VERSION {
            return Err(WorkspaceError::Invalid {
                path: path.display().to_string(),
                msg: format!("unsupported version {version}"),
            });
        }
        let workspace = wt.workspace.ok_or_else(|| WorkspaceError::Invalid {
            path: path.display().to_string(),
            msg: "missing workspace".to_string(),
        })?;
        if workspace.trim().is_empty() {
            return Err(WorkspaceError::Invalid {
                path: path.display().to_string(),
                msg: "workspace must be non-empty".to_string(),
            });
        }
        if wt.members.is_empty() {
            return Err(WorkspaceError::Invalid {
                path: path.display().to_string(),
                msg: "members must be non-empty".to_string(),
            });
        }

        let mut names = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut members = Vec::new();
        for m in wt.members {
            if m.name.trim().is_empty() || m.path.trim().is_empty() {
                return Err(WorkspaceError::Invalid {
                    path: path.display().to_string(),
                    msg: "member name/path must be non-empty".to_string(),
                });
            }
            if !names.insert(m.name.clone()) {
                return Err(WorkspaceError::Invalid {
                    path: path.display().to_string(),
                    msg: format!("duplicate member name {}", m.name),
                });
            }
            if !paths.insert(m.path.clone()) {
                return Err(WorkspaceError::Invalid {
                    path: path.display().to_string(),
                    msg: format!("duplicate member path {}", m.path),
                });
            }
            members.push(WorkspaceMember {
                name: m.name,
                path: m.path,
                role: m.role,
            });
        }

        let defaults_runtime_backend = wt
            .defaults
            .runtime_backend
            .as_deref()
            .map(parse_runtime_backend_profile)
            .transpose()
            .map_err(|msg| WorkspaceError::Invalid {
                path: path.display().to_string(),
                msg,
            })?;

        let defaults = WorkspaceDefaults {
            registry: wt.defaults.registry,
            policy: wt.defaults.policy,
            toolchain: wt.defaults.toolchain,
            runtime_backend: defaults_runtime_backend,
        };
        let mut profiles = BTreeMap::new();
        for (name, p) in wt.profiles {
            let runtime_backend = p
                .runtime_backend
                .as_deref()
                .map(parse_runtime_backend_profile)
                .transpose()
                .map_err(|msg| WorkspaceError::Invalid {
                    path: path.display().to_string(),
                    msg: format!("profile `{name}` {msg}"),
                })?;
            profiles.insert(
                name,
                WorkspaceProfile {
                    caps_policy: p.caps_policy,
                    registry: p.registry,
                    policy: p.policy,
                    toolchain: p.toolchain,
                    runtime_backend,
                },
            );
        }
        let mut tasks = BTreeMap::new();
        for (name, t) in wt.tasks {
            if t.cmd.trim().is_empty() {
                return Err(WorkspaceError::Invalid {
                    path: path.display().to_string(),
                    msg: format!("task {name} cmd must be non-empty"),
                });
            }
            tasks.insert(
                name,
                WorkspaceTask {
                    cmd: t.cmd,
                    file: t.file,
                    pkg: t.pkg,
                    args: t.args,
                },
            );
        }

        Ok(Self {
            version,
            workspace,
            members,
            defaults,
            profiles,
            tasks,
        })
    }

    pub fn to_toml_canonical(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("version = {GENESIS_WORKSPACE_VERSION}\n"));
        out.push_str(&format!("workspace = {}\n\n", toml_str(&self.workspace)));

        for m in &self.members {
            out.push_str("[[members]]\n");
            out.push_str(&format!("name = {}\n", toml_str(&m.name)));
            out.push_str(&format!("path = {}\n", toml_str(&m.path)));
            if let Some(role) = &m.role {
                out.push_str(&format!("role = {}\n", toml_str(role)));
            }
            out.push('\n');
        }

        out.push_str("[defaults]\n");
        if let Some(reg) = &self.defaults.registry {
            out.push_str(&format!("registry = {}\n", toml_str(reg)));
        }
        if let Some(policy) = &self.defaults.policy {
            out.push_str(&format!("policy = {}\n", toml_str(policy)));
        }
        if let Some(toolchain) = &self.defaults.toolchain {
            out.push_str(&format!("toolchain = {}\n", toml_str(toolchain)));
        }
        if let Some(runtime_backend) = &self.defaults.runtime_backend {
            out.push_str(&format!(
                "runtime_backend = {}\n",
                toml_str(runtime_backend)
            ));
        }
        out.push('\n');

        for (name, p) in &self.profiles {
            out.push_str(&format!("[profiles.{}]\n", toml_key(name)));
            if let Some(caps) = &p.caps_policy {
                out.push_str(&format!("caps_policy = {}\n", toml_str(caps)));
            }
            if let Some(reg) = &p.registry {
                out.push_str(&format!("registry = {}\n", toml_str(reg)));
            }
            if let Some(pol) = &p.policy {
                out.push_str(&format!("policy = {}\n", toml_str(pol)));
            }
            if let Some(toolchain) = &p.toolchain {
                out.push_str(&format!("toolchain = {}\n", toml_str(toolchain)));
            }
            if let Some(runtime_backend) = &p.runtime_backend {
                out.push_str(&format!(
                    "runtime_backend = {}\n",
                    toml_str(runtime_backend)
                ));
            }
            out.push('\n');
        }

        for (name, t) in &self.tasks {
            out.push_str(&format!("[tasks.{}]\n", toml_key(name)));
            out.push_str(&format!("cmd = {}\n", toml_str(&t.cmd)));
            if let Some(file) = &t.file {
                out.push_str(&format!("file = {}\n", toml_str(file)));
            }
            if let Some(pkg) = &t.pkg {
                out.push_str(&format!("pkg = {}\n", toml_str(pkg)));
            }
            if !t.args.is_empty() {
                out.push_str("args = [");
                for (i, a) in t.args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&toml_str(a));
                }
                out.push_str("]\n");
            }
            out.push('\n');
        }

        out
    }
}

fn toml_key(k: &str) -> String {
    format!("\"{}\"", k.replace('\\', "\\\\").replace('\"', "\\\""))
}

fn toml_str(s: &str) -> String {
    let mut out = String::new();
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn parse_runtime_backend_profile(raw: &str) -> Result<String, String> {
    normalize_runtime_backend_profile(raw).ok_or_else(|| {
        format!(
            "runtime_backend must be one of: {} | {} | {} | {} (or profile-* aliases), got `{}`",
            RUNTIME_BACKEND_HEADLESS,
            RUNTIME_BACKEND_GPU,
            RUNTIME_BACKEND_GFX,
            RUNTIME_BACKEND_BACKEND,
            raw
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        RUNTIME_BACKEND_BACKEND, RUNTIME_BACKEND_GFX, RUNTIME_BACKEND_GPU,
        RUNTIME_BACKEND_HEADLESS, WorkspaceConfig, normalize_runtime_backend_profile,
        runtime_backend_profile_is_compatible,
    };

    #[test]
    fn workspace_roundtrip_is_deterministic() {
        let mut ws = WorkspaceConfig::empty("root");
        ws.members.push(super::WorkspaceMember {
            name: "util".to_string(),
            path: "crates/util".to_string(),
            role: Some("lib".to_string()),
        });
        ws.defaults.registry = Some("gen://registry".to_string());
        ws.profiles.insert(
            "ci".to_string(),
            super::WorkspaceProfile {
                caps_policy: Some("caps.ci.toml".to_string()),
                registry: None,
                policy: Some("policy:strict".to_string()),
                toolchain: None,
                runtime_backend: Some(super::RUNTIME_BACKEND_HEADLESS.to_string()),
            },
        );
        ws.tasks.insert(
            "check".to_string(),
            super::WorkspaceTask {
                cmd: "test".to_string(),
                file: None,
                pkg: Some("package.toml".to_string()),
                args: vec!["--strict".to_string()],
            },
        );
        let s1 = ws.to_toml_canonical();
        let s2 = ws.to_toml_canonical();
        assert_eq!(s1, s2);
        let parsed =
            WorkspaceConfig::from_toml_str(std::path::Path::new("genesis.workspace.toml"), &s1)
                .unwrap();
        assert_eq!(parsed.workspace, "root");
        assert_eq!(parsed.members.len(), 2);
        assert!(parsed.tasks.contains_key("check"));
        assert_eq!(
            parsed
                .profiles
                .get("ci")
                .and_then(|p| p.runtime_backend.as_deref()),
            Some(RUNTIME_BACKEND_HEADLESS)
        );
    }

    #[test]
    fn version_is_required() {
        let body = r#"
workspace = "root"
[[members]]
name = "root"
path = "."
"#;
        let error =
            WorkspaceConfig::from_toml_str(std::path::Path::new("genesis.workspace.toml"), body)
                .unwrap_err()
                .to_string();
        assert!(error.contains("missing version"), "{error}");
    }

    #[test]
    fn runtime_backend_profile_normalization_and_compatibility() {
        assert_eq!(
            normalize_runtime_backend_profile("profile-headless").as_deref(),
            Some(RUNTIME_BACKEND_HEADLESS)
        );
        assert_eq!(
            normalize_runtime_backend_profile("GPU").as_deref(),
            Some(RUNTIME_BACKEND_GPU)
        );
        assert_eq!(
            normalize_runtime_backend_profile("profile-gfx").as_deref(),
            Some(RUNTIME_BACKEND_GFX)
        );
        assert_eq!(
            normalize_runtime_backend_profile("backend").as_deref(),
            Some(RUNTIME_BACKEND_BACKEND)
        );
        assert!(normalize_runtime_backend_profile("invalid").is_none());

        assert!(runtime_backend_profile_is_compatible(
            RUNTIME_BACKEND_HEADLESS,
            RUNTIME_BACKEND_HEADLESS
        ));
        assert!(runtime_backend_profile_is_compatible(
            RUNTIME_BACKEND_HEADLESS,
            RUNTIME_BACKEND_BACKEND
        ));
        assert!(runtime_backend_profile_is_compatible(
            RUNTIME_BACKEND_GPU,
            RUNTIME_BACKEND_BACKEND
        ));
        assert!(runtime_backend_profile_is_compatible(
            RUNTIME_BACKEND_GFX,
            RUNTIME_BACKEND_BACKEND
        ));
        assert!(!runtime_backend_profile_is_compatible(
            RUNTIME_BACKEND_BACKEND,
            RUNTIME_BACKEND_GPU
        ));
        assert!(!runtime_backend_profile_is_compatible(
            RUNTIME_BACKEND_GFX,
            RUNTIME_BACKEND_HEADLESS
        ));
    }
}
