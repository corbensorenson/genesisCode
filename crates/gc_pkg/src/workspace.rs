use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDefaults {
    pub registry: Option<String>,
    pub policy: Option<String>,
    pub toolchain: Option<String>,
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
            version: 1,
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
        let version = wt.version.unwrap_or(1);
        if version != 1 {
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

        let defaults = WorkspaceDefaults {
            registry: wt.defaults.registry,
            policy: wt.defaults.policy,
            toolchain: wt.defaults.toolchain,
        };
        let mut profiles = BTreeMap::new();
        for (name, p) in wt.profiles {
            profiles.insert(
                name,
                WorkspaceProfile {
                    caps_policy: p.caps_policy,
                    registry: p.registry,
                    policy: p.policy,
                    toolchain: p.toolchain,
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
        out.push_str("version = 1\n");
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

#[cfg(test)]
mod tests {
    use super::WorkspaceConfig;

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
    }
}
