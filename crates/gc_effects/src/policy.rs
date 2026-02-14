use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::EffectsError;

#[derive(Debug, Clone)]
pub struct CapsPolicy {
    ops: BTreeMap<String, OpPolicy>,
}

#[derive(Debug, Clone)]
pub struct OpPolicy {
    pub base_dir: Option<PathBuf>,
    pub create_dirs: bool,
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct CapsPolicyFile {
    version: Option<u64>,
    allow: Option<Vec<String>>,
    op: Option<BTreeMap<String, OpPolicyFile>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct OpPolicyFile {
    allow: Option<bool>,
    base_dir: Option<String>,
    create_dirs: Option<bool>,

    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

impl CapsPolicy {
    pub fn empty() -> Self {
        Self {
            ops: BTreeMap::new(),
        }
    }

    pub fn is_allowed(&self, op: &str) -> bool {
        self.ops.contains_key(op)
    }

    pub fn op_policy(&self, op: &str) -> Option<&OpPolicy> {
        self.ops.get(op)
    }

    pub fn from_toml_str(s: &str) -> Result<Self, EffectsError> {
        let file: CapsPolicyFile =
            toml::from_str(s).map_err(|e| EffectsError::Log(format!("caps.toml: {e}")))?;
        let _v = file.version.unwrap_or(1);

        let mut ops: BTreeMap<String, OpPolicy> = BTreeMap::new();

        if let Some(allow) = file.allow {
            for op in allow {
                ops.insert(
                    op,
                    OpPolicy {
                        base_dir: None,
                        create_dirs: false,
                        extra: BTreeMap::new(),
                    },
                );
            }
        }

        if let Some(op_table) = file.op {
            for (op, cfg) in op_table {
                let allow = cfg.allow.unwrap_or(true);
                if !allow {
                    ops.remove(&op);
                    continue;
                }
                ops.insert(
                    op,
                    OpPolicy {
                        base_dir: cfg.base_dir.map(PathBuf::from),
                        create_dirs: cfg.create_dirs.unwrap_or(false),
                        extra: cfg.extra,
                    },
                );
            }
        }

        Ok(Self { ops })
    }

    pub fn load(path: &Path) -> Result<Self, EffectsError> {
        let s = std::fs::read_to_string(path)?;
        Self::from_toml_str(&s)
    }
}

