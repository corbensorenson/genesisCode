use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LockError {
    #[error("lock io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("lock parse error: {path}: {msg}")]
    Parse { path: String, msg: String },

    #[error("lock invalid: {path}: {msg}")]
    Invalid { path: String, msg: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePolicy {
    Manual,
    Auto,
}

impl UpdatePolicy {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "manual" => Some(Self::Manual),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionStrategy {
    Pinned,
    TrackRef,
    TagPolicy,
}

impl ResolutionStrategy {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pinned" => Some(Self::Pinned),
            "track-ref" => Some(Self::TrackRef),
            "tag-policy" => Some(Self::TagPolicy),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pinned => "pinned",
            Self::TrackRef => "track-ref",
            Self::TagPolicy => "tag-policy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorKind {
    Commit,
    Snapshot,
    Ref,
    TagRef,
}

pub fn classify_selector(selector: &str) -> Option<SelectorKind> {
    let t = selector.trim();
    if let Some(rest) = t.strip_prefix("commit:") {
        if is_hex64(rest.trim()) {
            return Some(SelectorKind::Commit);
        }
        return None;
    }
    if let Some(rest) = t.strip_prefix("snapshot:") {
        if is_hex64(rest.trim()) {
            return Some(SelectorKind::Snapshot);
        }
        return None;
    }
    if let Some(rest) = t.strip_prefix("ref:") {
        let r = rest.trim();
        if r.starts_with("refs/tags/") {
            return Some(SelectorKind::TagRef);
        }
        if r.starts_with("refs/") {
            return Some(SelectorKind::Ref);
        }
        return None;
    }
    if t.starts_with("refs/tags/") {
        return Some(SelectorKind::TagRef);
    }
    if t.starts_with("refs/") {
        return Some(SelectorKind::Ref);
    }
    if is_hex64(t) {
        return Some(SelectorKind::Commit);
    }
    None
}

pub fn infer_strategy(selector: &str) -> ResolutionStrategy {
    match classify_selector(selector) {
        Some(SelectorKind::Commit) | Some(SelectorKind::Snapshot) | None => {
            ResolutionStrategy::Pinned
        }
        Some(SelectorKind::Ref) => ResolutionStrategy::TrackRef,
        Some(SelectorKind::TagRef) => ResolutionStrategy::TagPolicy,
    }
}

#[derive(Debug, Clone)]
pub struct Requirement {
    pub selector: String,
    pub update_policy: UpdatePolicy,
    pub registry: Option<String>,
    pub strategy: ResolutionStrategy,
    pub tag_policy: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LockedEntry {
    pub commit: Option<String>,
    pub snapshot: String,
    pub registry: Option<String>,
    pub source_selector: String,
    pub resolved_ref: Option<String>,
    pub exports_hash: Option<String>,
    pub environment_fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GenesisLock {
    pub version: u64,
    pub workspace: String,
    pub policy: String,
    pub registries: BTreeMap<String, String>,
    pub requirements: BTreeMap<String, Requirement>,
    pub locked: BTreeMap<String, LockedEntry>,
    pub artifacts: BTreeMap<String, String>,
}

pub fn default_lock_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("genesis.lock")
}

#[derive(Debug, Clone, Deserialize)]
struct LockToml {
    version: Option<u64>,
    workspace: Option<String>,
    policy: Option<String>,

    #[serde(default)]
    registries: BTreeMap<String, String>,

    #[serde(default)]
    requirements: BTreeMap<String, RequirementToml>,

    #[serde(default)]
    locked: BTreeMap<String, LockedToml>,

    #[serde(default)]
    artifacts: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RequirementToml {
    selector: String,
    #[serde(default)]
    update_policy: Option<String>,
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    strategy: Option<String>,
    #[serde(default)]
    tag_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LockedToml {
    #[serde(default)]
    commit: Option<String>,
    snapshot: String,
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    source_selector: Option<String>,
    #[serde(default)]
    resolved_ref: Option<String>,
    #[serde(default)]
    exports_hash: Option<String>,
    #[serde(default)]
    environment_fingerprint: Option<String>,
}

impl GenesisLock {
    pub fn empty(workspace: impl Into<String>) -> Self {
        Self {
            version: 2,
            workspace: workspace.into(),
            policy: "policy:default-v0.1".to_string(),
            registries: BTreeMap::new(),
            requirements: BTreeMap::new(),
            locked: BTreeMap::new(),
            artifacts: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, LockError> {
        let s = std::fs::read_to_string(path)?;
        Self::from_toml_str(path, &s)
    }

    pub fn from_toml_str(path: &Path, s: &str) -> Result<Self, LockError> {
        let lt: LockToml = toml::from_str(s).map_err(|e| LockError::Parse {
            path: path.display().to_string(),
            msg: e.to_string(),
        })?;

        let version = lt.version.unwrap_or(1);
        if version != 1 && version != 2 {
            return Err(LockError::Invalid {
                path: path.display().to_string(),
                msg: format!("unsupported version {version}"),
            });
        }
        let workspace = lt.workspace.ok_or_else(|| LockError::Invalid {
            path: path.display().to_string(),
            msg: "missing workspace".to_string(),
        })?;
        let policy = lt
            .policy
            .unwrap_or_else(|| "policy:default-v0.1".to_string());

        let mut requirements = BTreeMap::new();
        for (name, r) in lt.requirements {
            let up = r
                .update_policy
                .as_deref()
                .and_then(UpdatePolicy::from_str)
                .unwrap_or(UpdatePolicy::Manual);
            let strategy = match r.strategy.as_deref().and_then(ResolutionStrategy::from_str) {
                Some(s) => s,
                None => infer_strategy(&r.selector),
            };
            if matches!(strategy, ResolutionStrategy::TagPolicy)
                && !matches!(classify_selector(&r.selector), Some(SelectorKind::TagRef))
            {
                return Err(LockError::Invalid {
                    path: path.display().to_string(),
                    msg: format!(
                        "requirement {name} uses strategy tag-policy but selector is not refs/tags/*"
                    ),
                });
            }
            requirements.insert(
                name,
                Requirement {
                    selector: r.selector,
                    update_policy: up,
                    registry: r.registry,
                    strategy,
                    tag_policy: r.tag_policy,
                },
            );
        }

        let mut locked = BTreeMap::new();
        for (name, l) in lt.locked {
            locked.insert(
                name,
                LockedEntry {
                    commit: l.commit,
                    snapshot: l.snapshot,
                    registry: l.registry,
                    source_selector: l.source_selector.unwrap_or_default(),
                    resolved_ref: l.resolved_ref,
                    exports_hash: l.exports_hash,
                    environment_fingerprint: l.environment_fingerprint,
                },
            );
        }

        Ok(Self {
            version,
            workspace,
            policy,
            registries: lt.registries,
            requirements,
            locked,
            artifacts: lt.artifacts,
        })
    }

    pub fn to_toml_canonical(&self) -> String {
        let version = if self.version < 2 && has_v2_requirements(&self.requirements) {
            2
        } else if self.version == 0 {
            2
        } else {
            self.version
        };
        let mut out = String::new();
        out.push_str(&format!("version = {version}\n"));
        out.push_str(&format!("workspace = {}\n", toml_str(&self.workspace)));
        out.push_str(&format!("policy = {}\n", toml_str(&self.policy)));
        out.push('\n');

        if !self.registries.is_empty() {
            out.push_str("[registries]\n");
            for (k, v) in &self.registries {
                out.push_str(&format!("{k} = {}\n", toml_str(v)));
            }
            out.push('\n');
        }

        out.push_str("[requirements]\n");
        for (name, r) in &self.requirements {
            out.push_str(&format!(
                "{} = {{ selector = {}, update_policy = {}, registry = {}, strategy = {}",
                toml_key(name),
                toml_str(&r.selector),
                toml_str(r.update_policy.as_str()),
                toml_str(r.registry.as_deref().unwrap_or("default")),
                toml_str(r.strategy.as_str()),
            ));
            if let Some(tp) = &r.tag_policy {
                out.push_str(&format!(", tag_policy = {}", toml_str(tp)));
            }
            out.push_str(" }\n");
        }
        out.push('\n');

        out.push_str("[locked]\n");
        for (name, l) in &self.locked {
            out.push_str(&format!("{} = {{ ", toml_key(name)));
            let mut first = true;
            if let Some(c) = &l.commit {
                first = false;
                out.push_str(&format!("commit = {}", toml_str(c)));
            }
            if !first {
                out.push_str(", ");
            }
            out.push_str(&format!("snapshot = {}", toml_str(&l.snapshot)));
            if let Some(r) = &l.registry {
                out.push_str(&format!(", registry = {}", toml_str(r)));
            }
            if !l.source_selector.is_empty() {
                out.push_str(&format!(
                    ", source_selector = {}",
                    toml_str(&l.source_selector)
                ));
            }
            if let Some(rr) = &l.resolved_ref {
                out.push_str(&format!(", resolved_ref = {}", toml_str(rr)));
            }
            if let Some(x) = &l.exports_hash {
                out.push_str(&format!(", exports_hash = {}", toml_str(x)));
            }
            if let Some(env_fp) = &l.environment_fingerprint {
                out.push_str(&format!(", environment_fingerprint = {}", toml_str(env_fp)));
            }
            out.push_str(" }\n");
        }
        out.push('\n');

        if !self.artifacts.is_empty() {
            out.push_str("[artifacts]\n");
            for (k, v) in &self.artifacts {
                out.push_str(&format!("{k} = {}\n", toml_str(v)));
            }
            out.push('\n');
        }

        out
    }

    pub fn set_requirement(
        &mut self,
        name: &str,
        selector: &str,
        update_policy: UpdatePolicy,
        registry: Option<String>,
    ) {
        self.set_requirement_with_metadata(name, selector, update_policy, registry, None, None);
    }

    pub fn set_requirement_with_metadata(
        &mut self,
        name: &str,
        selector: &str,
        update_policy: UpdatePolicy,
        registry: Option<String>,
        strategy: Option<ResolutionStrategy>,
        tag_policy: Option<String>,
    ) {
        let strategy = strategy.unwrap_or_else(|| infer_strategy(selector));
        let tag_policy = if matches!(strategy, ResolutionStrategy::TagPolicy) {
            Some(tag_policy.unwrap_or_else(|| "exact".to_string()))
        } else {
            None
        };
        self.requirements.insert(
            name.to_string(),
            Requirement {
                selector: selector.to_string(),
                update_policy,
                registry,
                strategy,
                tag_policy,
            },
        );
    }

    pub fn requirements_missing_locks(&self) -> Vec<String> {
        let mut out = Vec::new();
        for name in self.requirements.keys() {
            if !self.locked.contains_key(name) {
                out.push(name.clone());
            }
        }
        out
    }
}

fn toml_key(k: &str) -> String {
    // Quote keys to avoid edge cases with '-' or '.' and to match examples.
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
            c if c.is_control() => {
                // TOML basic strings support \uXXXX escapes.
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn has_v2_requirements(requirements: &BTreeMap<String, Requirement>) -> bool {
    requirements.values().any(|r| {
        !matches!(r.strategy, ResolutionStrategy::Pinned)
            || r.tag_policy.is_some()
            || matches!(
                classify_selector(&r.selector),
                Some(SelectorKind::Ref | SelectorKind::TagRef)
            )
    })
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.as_bytes().iter().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{
        GenesisLock, LockError, ResolutionStrategy, SelectorKind, UpdatePolicy, classify_selector,
        infer_strategy,
    };

    #[test]
    fn canonical_writer_is_deterministic_and_roundtrips() {
        let mut l = GenesisLock::empty("ws");
        l.policy = "policy:x".to_string();
        l.registries
            .insert("default".to_string(), "gen://local".to_string());
        l.set_requirement(
            "b",
            "snapshot:00",
            UpdatePolicy::Manual,
            Some("default".to_string()),
        );
        l.set_requirement(
            "a",
            "refs/heads/main",
            UpdatePolicy::Auto,
            Some("default".to_string()),
        );
        l.locked.insert(
            "a".to_string(),
            super::LockedEntry {
                commit: Some("11".to_string()),
                snapshot: "22".to_string(),
                registry: Some("default".to_string()),
                source_selector: "refs/heads/main".to_string(),
                resolved_ref: Some("refs/heads/main".to_string()),
                exports_hash: None,
                environment_fingerprint: None,
            },
        );
        let s1 = l.to_toml_canonical();
        let s2 = l.to_toml_canonical();
        assert_eq!(s1, s2);

        // Parsing uses TOML, but we should at least be able to parse what we wrote.
        let parsed = GenesisLock::from_toml_str(std::path::Path::new("genesis.lock"), &s1)
            .map_err(|e| format!("{e}"))
            .unwrap();
        assert_eq!(parsed.version, 2);
        assert_eq!(parsed.workspace, "ws");
        assert_eq!(parsed.policy, "policy:x");
        assert!(parsed.requirements.contains_key("a"));
        assert!(parsed.requirements.contains_key("b"));
        assert_eq!(
            parsed.requirements.get("a").map(|r| r.strategy),
            Some(ResolutionStrategy::TrackRef)
        );
        assert_eq!(
            parsed.requirements.get("b").map(|r| r.strategy),
            Some(ResolutionStrategy::Pinned)
        );
    }

    #[test]
    fn rejects_unsupported_version() {
        let s = "version = 3\nworkspace = \"w\"\npolicy = \"p\"\n[requirements]\n[locked]\n";
        let e = GenesisLock::from_toml_str(std::path::Path::new("genesis.lock"), s).unwrap_err();
        match e {
            LockError::Invalid { .. } => {}
            other => panic!("expected invalid, got {other:?}"),
        }
    }

    #[test]
    fn selector_classification_and_strategy_inference_are_stable() {
        assert_eq!(
            classify_selector(
                "commit:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ),
            Some(SelectorKind::Commit)
        );
        assert_eq!(
            classify_selector(
                "snapshot:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ),
            Some(SelectorKind::Snapshot)
        );
        assert_eq!(
            classify_selector("refs/heads/main"),
            Some(SelectorKind::Ref)
        );
        assert_eq!(
            classify_selector("refs/tags/v1.2.3"),
            Some(SelectorKind::TagRef)
        );
        assert_eq!(
            infer_strategy("refs/heads/main"),
            ResolutionStrategy::TrackRef
        );
        assert_eq!(
            infer_strategy("refs/tags/v1.2.3"),
            ResolutionStrategy::TagPolicy
        );
        assert_eq!(
            infer_strategy(
                "snapshot:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ),
            ResolutionStrategy::Pinned
        );
    }

    #[test]
    fn rejects_tag_policy_on_non_tag_selector() {
        let s = r#"
version = 2
workspace = "w"
policy = "p"

[requirements]
"dep" = { selector = "refs/heads/main", update_policy = "manual", registry = "default", strategy = "tag-policy" }

[locked]
"#;
        let e = GenesisLock::from_toml_str(std::path::Path::new("genesis.lock"), s).unwrap_err();
        match e {
            LockError::Invalid { .. } => {}
            other => panic!("expected invalid, got {other:?}"),
        }
    }
}
