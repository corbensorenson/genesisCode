use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
        let v: toml::Value =
            toml::from_str(s).map_err(|e| EffectsError::Log(format!("caps.toml: {e}")))?;
        let tbl = v.as_table().ok_or_else(|| {
            EffectsError::Log("caps.toml: top-level value must be a table".to_string())
        })?;

        let _version = tbl.get("version").and_then(|v| v.as_integer()).unwrap_or(1);

        let mut ops: BTreeMap<String, OpPolicy> = BTreeMap::new();

        // Baseline allowlist.
        if let Some(arr) = tbl.get("allow").and_then(|v| v.as_array()) {
            for op in arr {
                let op = op.as_str().ok_or_else(|| {
                    EffectsError::Log("caps.toml: allow entries must be strings".to_string())
                })?;
                ops.insert(
                    op.to_string(),
                    OpPolicy {
                        base_dir: None,
                        create_dirs: false,
                        extra: BTreeMap::new(),
                    },
                );
            }
        }

        // Per-op configuration is accepted in two equivalent encodings:
        // - canonical: [op."<op-symbol>"] tables (preferred)
        // - legacy/shortcut: ["<op-symbol>"] tables at the top level
        //
        // Both are merged into `ops` with allow/remove semantics.
        if let Some(op_tbl) = tbl.get("op").and_then(|v| v.as_table()) {
            for (op, cfg) in op_tbl {
                apply_op_cfg(&mut ops, op, cfg)?;
            }
        }

        for (k, v) in tbl {
            if k == "version" || k == "allow" || k == "op" {
                continue;
            }
            if let Some(_cfg_tbl) = v.as_table() {
                apply_op_cfg(&mut ops, k, v)?;
            }
        }

        Ok(Self { ops })
    }

    pub fn load(path: &Path) -> Result<Self, EffectsError> {
        let s = std::fs::read_to_string(path)?;
        let mut pol = Self::from_toml_str(&s)?;
        pol.resolve_relative_paths(path.parent().unwrap_or_else(|| Path::new(".")));
        Ok(pol)
    }

    fn resolve_relative_paths(&mut self, base: &Path) {
        for p in self.ops.values_mut() {
            if let Some(bd) = &p.base_dir
                && bd.is_relative()
            {
                p.base_dir = Some(base.join(bd));
            }
        }
    }
}

fn apply_op_cfg(
    ops: &mut BTreeMap<String, OpPolicy>,
    op: &str,
    cfg: &toml::Value,
) -> Result<(), EffectsError> {
    let tbl = cfg
        .as_table()
        .ok_or_else(|| EffectsError::Log(format!("caps.toml: op {op} config must be a table")))?;

    let allow = tbl.get("allow").and_then(|v| v.as_bool()).unwrap_or(true);
    if !allow {
        ops.remove(op);
        return Ok(());
    }

    let base_dir = tbl
        .get("base_dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    let create_dirs = tbl
        .get("create_dirs")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut extra = BTreeMap::new();
    for (k, v) in tbl {
        if k == "allow" || k == "base_dir" || k == "create_dirs" {
            continue;
        }
        extra.insert(k.clone(), v.clone());
    }

    ops.insert(
        op.to_string(),
        OpPolicy {
            base_dir,
            create_dirs,
            extra,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CapsPolicy;

    #[test]
    fn supports_legacy_top_level_op_tables() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["io/fs::read"]

["io/fs::read"]
base_dir = "./x"
"#,
        )
        .unwrap();
        assert!(p.is_allowed("io/fs::read"));
        assert!(p.op_policy("io/fs::read").unwrap().base_dir.is_some());
    }

    #[test]
    fn supports_canonical_op_table() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["io/fs::read"]

[op."io/fs::read"]
base_dir = "./x"
"#,
        )
        .unwrap();
        assert!(p.is_allowed("io/fs::read"));
        assert!(p.op_policy("io/fs::read").unwrap().base_dir.is_some());
    }
}
