use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::EffectsError;

#[derive(Debug, Clone)]
pub struct CapsPolicy {
    ops: BTreeMap<String, OpPolicy>,
    pub log: LogPolicy,
    pub store: StorePolicy,
    pub refs: RefsPolicy,
    pub task: TaskPolicy,
}

#[derive(Debug, Clone)]
pub struct LogPolicy {
    /// Maximum number of bytes to inline inside `.gclog` `:resp`.
    ///
    /// When set and a response exceeds the limit, the runner stores the response in the
    /// content-addressed store and records an artifact reference in the log.
    pub inline_max_bytes: Option<usize>,

    /// Directory containing content-addressed artifacts for logs (defaults to `<caps-dir>/.genesis/store`
    /// when `inline_max_bytes` is set and `store_dir` is omitted).
    pub store_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct StorePolicy {
    /// Content-addressed store directory used by `core/store::*` capabilities.
    pub dir: Option<PathBuf>,

    /// Optional remote registry base used as a read-through source for `core/store::{has,get}`.
    ///
    /// This is secure-by-default: if `remote` is set, the runner still requires `remote_allow`
    /// to be non-empty and to allow the normalized base URL prefix.
    pub remote: Option<String>,

    /// Allowlist of remote base URL prefixes permitted for `store.remote`.
    pub remote_allow: Vec<String>,

    /// If true, `http://` remotes are permitted (default false).
    pub allow_http: bool,
}

#[derive(Debug, Clone)]
pub struct RefsPolicy {
    /// Local refs database file used by `core/refs::*` capabilities.
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct TaskPolicy {
    pub max_tasks: Option<u64>,
    pub max_workers: Option<u64>,
    pub max_queue: Option<u64>,
    pub max_steps_per_task: Option<u64>,
    pub max_time_ms_per_task: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct OpPolicy {
    pub base_dir: Option<PathBuf>,
    pub create_dirs: bool,
    pub timeout_ms: Option<u64>,
    pub log_inline_max_bytes: Option<usize>,
    pub extra: BTreeMap<String, toml::Value>,
}

impl CapsPolicy {
    pub fn empty() -> Self {
        Self {
            ops: BTreeMap::new(),
            log: LogPolicy {
                inline_max_bytes: None,
                store_dir: None,
            },
            store: StorePolicy {
                dir: None,
                remote: None,
                remote_allow: Vec::new(),
                allow_http: false,
            },
            refs: RefsPolicy { path: None },
            task: TaskPolicy {
                max_tasks: None,
                max_workers: None,
                max_queue: None,
                max_steps_per_task: None,
                max_time_ms_per_task: None,
            },
        }
    }

    pub fn is_allowed(&self, op: &str) -> bool {
        self.ops.contains_key(op)
            || low_level_aliases(op)
                .iter()
                .any(|alias| self.ops.contains_key(*alias))
    }

    pub fn op_policy(&self, op: &str) -> Option<&OpPolicy> {
        self.ops.get(op).or_else(|| {
            low_level_aliases(op)
                .iter()
                .find_map(|alias| self.ops.get(*alias))
        })
    }

    pub fn inline_max_bytes_for(&self, op: &str) -> Option<usize> {
        if let Some(p) = self.ops.get(op)
            && let Some(x) = p.log_inline_max_bytes
        {
            return Some(x);
        }
        self.log.inline_max_bytes
    }

    pub fn store_dir(&self) -> Option<&Path> {
        self.log.store_dir.as_deref()
    }

    pub fn artifact_store_dir(&self) -> Option<&Path> {
        self.store.dir.as_deref().or(self.log.store_dir.as_deref())
    }

    pub fn refs_db_path(&self) -> Option<&Path> {
        self.refs.path.as_deref()
    }

    pub fn from_toml_str(s: &str) -> Result<Self, EffectsError> {
        let v: toml::Value =
            toml::from_str(s).map_err(|e| EffectsError::Log(format!("caps.toml: {e}")))?;
        let tbl = v.as_table().ok_or_else(|| {
            EffectsError::Log("caps.toml: top-level value must be a table".to_string())
        })?;

        let _version = tbl.get("version").and_then(|v| v.as_integer()).unwrap_or(1);
        let mut ops: BTreeMap<String, OpPolicy> = BTreeMap::new();
        let log = parse_log_policy(tbl)?;
        let store = parse_store_policy(tbl)?;
        let refs = parse_refs_policy(tbl)?;
        let task = parse_task_policy(tbl)?;

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
                        timeout_ms: None,
                        log_inline_max_bytes: None,
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
            if k == "version"
                || k == "allow"
                || k == "op"
                || k == "log"
                || k == "store"
                || k == "refs"
                || k == "task"
            {
                continue;
            }
            if let Some(_cfg_tbl) = v.as_table() {
                apply_op_cfg(&mut ops, k, v)?;
            }
        }

        Ok(Self {
            ops,
            log,
            store,
            refs,
            task,
        })
    }

    pub fn load(path: &Path) -> Result<Self, EffectsError> {
        let s = std::fs::read_to_string(path)?;
        let mut pol = Self::from_toml_str(&s)?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        pol.resolve_relative_paths(base);
        if pol.log.inline_max_bytes.is_some() && pol.log.store_dir.is_none() {
            pol.log.store_dir = Some(base.join(".genesis").join("store"));
        }
        if pol.store.dir.is_none() {
            pol.store.dir = Some(base.join(".genesis").join("store"));
        }
        if pol.refs.path.is_none() {
            pol.refs.path = Some(base.join(".genesis").join("refs.gc"));
        }
        Ok(pol)
    }

    fn resolve_relative_paths(&mut self, base: &Path) {
        if let Some(sd) = &self.log.store_dir
            && sd.is_relative()
        {
            self.log.store_dir = Some(base.join(sd));
        }
        if let Some(sd) = &self.store.dir
            && sd.is_relative()
        {
            self.store.dir = Some(base.join(sd));
        }
        if let Some(rp) = &self.refs.path
            && rp.is_relative()
        {
            self.refs.path = Some(base.join(rp));
        }
        for p in self.ops.values_mut() {
            if let Some(bd) = &p.base_dir
                && bd.is_relative()
            {
                p.base_dir = Some(base.join(bd));
            }
        }
    }
}

fn low_level_aliases(op: &str) -> &'static [&'static str] {
    match op {
        // Internal low-level alias: `pkg snapshot` checks call `load-package`.
        "core/pkg-low::load-package" => &["core/pkg-low::snapshot"],
        _ => &[],
    }
}

fn parse_log_policy(tbl: &toml::value::Table) -> Result<LogPolicy, EffectsError> {
    let Some(v) = tbl.get("log") else {
        return Ok(LogPolicy {
            inline_max_bytes: None,
            store_dir: None,
        });
    };
    let log_tbl = v
        .as_table()
        .ok_or_else(|| EffectsError::Log("caps.toml: log must be a table".to_string()))?;

    let inline_max_bytes = match log_tbl.get("inline_max_bytes") {
        None => None,
        Some(x) => {
            let n = x.as_integer().ok_or_else(|| {
                EffectsError::Log("caps.toml: log.inline_max_bytes must be an integer".to_string())
            })?;
            if n <= 0 { None } else { Some(n as usize) }
        }
    };
    let store_dir = log_tbl
        .get("store_dir")
        .and_then(|x| x.as_str())
        .map(PathBuf::from);

    Ok(LogPolicy {
        inline_max_bytes,
        store_dir,
    })
}

fn parse_store_policy(tbl: &toml::value::Table) -> Result<StorePolicy, EffectsError> {
    let Some(v) = tbl.get("store") else {
        return Ok(StorePolicy {
            dir: None,
            remote: None,
            remote_allow: Vec::new(),
            allow_http: false,
        });
    };
    let store_tbl = v
        .as_table()
        .ok_or_else(|| EffectsError::Log("caps.toml: store must be a table".to_string()))?;
    let dir = store_tbl
        .get("dir")
        .and_then(|x| x.as_str())
        .map(PathBuf::from);
    let remote = store_tbl
        .get("remote")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let allow_http = store_tbl
        .get("allow_http")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let remote_allow = match store_tbl.get("remote_allow") {
        None => Vec::new(),
        Some(v) => {
            let arr = v.as_array().ok_or_else(|| {
                EffectsError::Log("caps.toml: store.remote_allow must be an array".to_string())
            })?;
            let mut out = Vec::new();
            for x in arr {
                let s = x.as_str().ok_or_else(|| {
                    EffectsError::Log(
                        "caps.toml: store.remote_allow entries must be strings".to_string(),
                    )
                })?;
                out.push(s.to_string());
            }
            out
        }
    };
    Ok(StorePolicy {
        dir,
        remote,
        remote_allow,
        allow_http,
    })
}

fn parse_refs_policy(tbl: &toml::value::Table) -> Result<RefsPolicy, EffectsError> {
    let Some(v) = tbl.get("refs") else {
        return Ok(RefsPolicy { path: None });
    };
    let refs_tbl = v
        .as_table()
        .ok_or_else(|| EffectsError::Log("caps.toml: refs must be a table".to_string()))?;
    let path = refs_tbl
        .get("path")
        .and_then(|x| x.as_str())
        .map(PathBuf::from);
    Ok(RefsPolicy { path })
}

fn parse_task_policy(tbl: &toml::value::Table) -> Result<TaskPolicy, EffectsError> {
    let Some(v) = tbl.get("task") else {
        return Ok(TaskPolicy {
            max_tasks: None,
            max_workers: None,
            max_queue: None,
            max_steps_per_task: None,
            max_time_ms_per_task: None,
        });
    };
    let task_tbl = v
        .as_table()
        .ok_or_else(|| EffectsError::Log("caps.toml: task must be a table".to_string()))?;

    fn parse_u64_opt(
        task_tbl: &toml::value::Table,
        key: &str,
    ) -> Result<Option<u64>, EffectsError> {
        match task_tbl.get(key) {
            None => Ok(None),
            Some(v) => {
                let n = v.as_integer().ok_or_else(|| {
                    EffectsError::Log(format!("caps.toml: task.{key} must be an integer"))
                })?;
                if n < 0 {
                    return Err(EffectsError::Log(format!(
                        "caps.toml: task.{key} must be >= 0"
                    )));
                }
                Ok(Some(n as u64))
            }
        }
    }

    Ok(TaskPolicy {
        max_tasks: parse_u64_opt(task_tbl, "max_tasks")?,
        max_workers: parse_u64_opt(task_tbl, "max_workers")?,
        max_queue: parse_u64_opt(task_tbl, "max_queue")?,
        max_steps_per_task: parse_u64_opt(task_tbl, "max_steps_per_task")?,
        max_time_ms_per_task: parse_u64_opt(task_tbl, "max_time_ms_per_task")?,
    })
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
    let timeout_ms = match tbl.get("timeout_ms") {
        None => None,
        Some(v) => Some(
            v.as_integer()
                .ok_or_else(|| {
                    EffectsError::Log(format!("caps.toml: op {op} timeout_ms must be integer"))
                })?
                .max(0) as u64,
        ),
    };
    let log_inline_max_bytes = match tbl.get("log_inline_max_bytes") {
        None => None,
        Some(x) => {
            let n = x.as_integer().ok_or_else(|| {
                EffectsError::Log(format!(
                    "caps.toml: op {op} log_inline_max_bytes must be integer"
                ))
            })?;
            if n <= 0 { None } else { Some(n as usize) }
        }
    };

    let mut extra = BTreeMap::new();
    for (k, v) in tbl {
        if k == "allow"
            || k == "base_dir"
            || k == "create_dirs"
            || k == "timeout_ms"
            || k == "log_inline_max_bytes"
        {
            continue;
        }
        extra.insert(k.clone(), v.clone());
    }

    ops.insert(
        op.to_string(),
        OpPolicy {
            base_dir,
            create_dirs,
            timeout_ms,
            log_inline_max_bytes,
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

    #[test]
    fn parses_log_policy_and_resolves_defaults() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["sys/time::now"]

[log]
inline_max_bytes = 123
store_dir = "./s"
"#,
        )
        .unwrap();
        assert_eq!(p.log.inline_max_bytes, Some(123));
        assert!(p.log.store_dir.is_some());
    }

    #[test]
    fn per_op_inline_max_overrides_global() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["sys/time::now"]

[log]
inline_max_bytes = 10

[op."sys/time::now"]
log_inline_max_bytes = 5
"#,
        )
        .unwrap();
        assert_eq!(p.inline_max_bytes_for("sys/time::now"), Some(5));
    }

    #[test]
    fn parses_task_policy_limits() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["core/task::await"]

[task]
max_tasks = 12
max_workers = 4
max_queue = 16
max_steps_per_task = 20
max_time_ms_per_task = 50
"#,
        )
        .unwrap();
        assert_eq!(p.task.max_tasks, Some(12));
        assert_eq!(p.task.max_workers, Some(4));
        assert_eq!(p.task.max_queue, Some(16));
        assert_eq!(p.task.max_steps_per_task, Some(20));
        assert_eq!(p.task.max_time_ms_per_task, Some(50));
    }

    #[test]
    fn supports_pkg_snapshot_low_level_alias_for_low_level_loader() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["core/pkg-low::snapshot"]

[op."core/pkg-low::snapshot"]
base_dir = "."
"#,
        )
        .unwrap();
        assert!(p.is_allowed("core/pkg-low::load-package"));
        assert!(p.op_policy("core/pkg-low::load-package").is_some());
    }

    #[test]
    fn legacy_high_level_ops_do_not_authorize_low_level_ops() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["core/pkg::lock"]

[op."core/pkg::lock"]
base_dir = "."
"#,
        )
        .unwrap();
        assert!(!p.is_allowed("core/pkg-low::lock"));
        assert!(p.op_policy("core/pkg-low::lock").is_none());
    }
}
