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

    /// Optional cumulative per-run byte budget for response artifacts externalized by the logger.
    pub max_artifact_bytes_per_run: Option<usize>,
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

    /// Optional cumulative per-run byte budget for content-addressed store writes.
    pub max_run_bytes: Option<usize>,

    /// Optional bearer token presented to remote registries.
    pub auth_token: Option<String>,

    /// Optional env var name containing bearer token for remote registries.
    pub auth_token_env: Option<String>,

    /// Optional username for HTTP basic auth against remote registries.
    pub basic_username: Option<String>,

    /// Optional inline password for HTTP basic auth.
    pub basic_password: Option<String>,

    /// Optional env var name containing HTTP basic auth password.
    pub basic_password_env: Option<String>,

    /// Optional PEM path for additional trusted CA roots used by remote TLS.
    pub mtls_ca_pem: Option<PathBuf>,

    /// Optional PEM path for client identity used by mTLS.
    pub mtls_identity_pem: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct RefsPolicy {
    /// Local refs database file used by `core/refs::*` capabilities.
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct TaskPolicy {
    pub default_workers: u64,
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
                max_artifact_bytes_per_run: None,
            },
            store: StorePolicy {
                dir: None,
                remote: None,
                remote_allow: Vec::new(),
                allow_http: false,
                max_run_bytes: None,
                auth_token: None,
                auth_token_env: None,
                basic_username: None,
                basic_password: None,
                basic_password_env: None,
                mtls_ca_pem: None,
                mtls_identity_pem: None,
            },
            refs: RefsPolicy { path: None },
            task: TaskPolicy {
                default_workers: 1,
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
                if let Some(replacement) = retired_high_level_op_replacement(op) {
                    return Err(EffectsError::Log(format!(
                        "caps.toml: legacy high-level op `{op}` is retired; use `{replacement}`"
                    )));
                }
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
        if let Some(ca) = &self.store.mtls_ca_pem
            && ca.is_relative()
        {
            self.store.mtls_ca_pem = Some(base.join(ca));
        }
        if let Some(id) = &self.store.mtls_identity_pem
            && id.is_relative()
        {
            self.store.mtls_identity_pem = Some(base.join(id));
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
            for key in ["mtls_ca_pem", "mtls_identity_pem"] {
                if let Some(v) = p.extra.get_mut(key)
                    && let Some(s) = v.as_str()
                {
                    let pb = PathBuf::from(s);
                    if pb.is_relative() {
                        *v = toml::Value::String(base.join(pb).to_string_lossy().to_string());
                    }
                }
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

fn retired_high_level_op_replacement(op: &str) -> Option<&'static str> {
    match op {
        "core/pkg::init" => Some("core/pkg-low::init"),
        "core/pkg::add" => Some("core/pkg-low::add"),
        "core/pkg::list" => Some("core/pkg-low::list"),
        "core/pkg::info" => Some("core/pkg-low::info"),
        "core/pkg::lock" => Some("core/pkg-low::lock"),
        "core/pkg::update" => Some("core/pkg-low::update"),
        "core/pkg::install" => Some("core/pkg-low::install"),
        "core/pkg::verify" => Some("core/pkg-low::verify"),
        "core/pkg::snapshot" => Some("core/pkg-low::snapshot"),
        "core/pkg::publish" => Some("core/pkg-low::publish"),
        "core/gpk::export" => Some("core/gpk-low::export"),
        "core/gpk::import" => Some("core/gpk-low::import"),
        "core/gc::plan" => Some("core/gc-low::plan"),
        "core/gc::run" => Some("core/gc-low::run"),
        "core/gc::pin" => Some("core/gc-low::pin"),
        "core/gc::unpin" => Some("core/gc-low::unpin"),
        "core/gc::purge" => Some("core/gc-low::purge"),
        "core/vcs::log" => Some("core/vcs-low::log"),
        "core/vcs::blame" => Some("core/vcs-low::blame"),
        "core/vcs::why" => Some("core/vcs-low::why"),
        "core/vcs::diff" => Some("core/vcs-low::diff"),
        "core/vcs::apply" => Some("core/vcs-low::apply"),
        "core/vcs::merge3" => Some("core/vcs-low::merge3"),
        "core/vcs::resolve-conflict" => Some("core/vcs-low::resolve-conflict"),
        _ => None,
    }
}

fn parse_log_policy(tbl: &toml::value::Table) -> Result<LogPolicy, EffectsError> {
    let Some(v) = tbl.get("log") else {
        return Ok(LogPolicy {
            inline_max_bytes: None,
            store_dir: None,
            max_artifact_bytes_per_run: None,
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
            if n <= 0 {
                None
            } else {
                Some(usize::try_from(n).map_err(|_| {
                    EffectsError::Log(
                        "caps.toml: log.inline_max_bytes is too large for this platform"
                            .to_string(),
                    )
                })?)
            }
        }
    };
    let store_dir = log_tbl
        .get("store_dir")
        .and_then(|x| x.as_str())
        .map(PathBuf::from);
    let max_artifact_bytes_per_run = match log_tbl.get("max_artifact_bytes_per_run") {
        None => None,
        Some(x) => {
            let n = x.as_integer().ok_or_else(|| {
                EffectsError::Log(
                    "caps.toml: log.max_artifact_bytes_per_run must be an integer".to_string(),
                )
            })?;
            if n <= 0 {
                None
            } else {
                Some(usize::try_from(n).map_err(|_| {
                    EffectsError::Log(
                        "caps.toml: log.max_artifact_bytes_per_run is too large for this platform"
                            .to_string(),
                    )
                })?)
            }
        }
    };

    Ok(LogPolicy {
        inline_max_bytes,
        store_dir,
        max_artifact_bytes_per_run,
    })
}

fn parse_store_policy(tbl: &toml::value::Table) -> Result<StorePolicy, EffectsError> {
    let Some(v) = tbl.get("store") else {
        return Ok(StorePolicy {
            dir: None,
            remote: None,
            remote_allow: Vec::new(),
            allow_http: false,
            max_run_bytes: None,
            auth_token: None,
            auth_token_env: None,
            basic_username: None,
            basic_password: None,
            basic_password_env: None,
            mtls_ca_pem: None,
            mtls_identity_pem: None,
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
    let auth_token = store_tbl
        .get("auth_token")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let auth_token_env = store_tbl
        .get("auth_token_env")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let basic_username = store_tbl
        .get("basic_username")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let basic_password = store_tbl
        .get("basic_password")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let basic_password_env = store_tbl
        .get("basic_password_env")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let mtls_ca_pem = store_tbl
        .get("mtls_ca_pem")
        .and_then(|x| x.as_str())
        .map(PathBuf::from);
    let mtls_identity_pem = store_tbl
        .get("mtls_identity_pem")
        .and_then(|x| x.as_str())
        .map(PathBuf::from);
    let allow_http = store_tbl
        .get("allow_http")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let max_run_bytes = match store_tbl.get("max_run_bytes") {
        None => None,
        Some(x) => {
            let n = x.as_integer().ok_or_else(|| {
                EffectsError::Log("caps.toml: store.max_run_bytes must be an integer".to_string())
            })?;
            if n <= 0 {
                None
            } else {
                Some(usize::try_from(n).map_err(|_| {
                    EffectsError::Log(
                        "caps.toml: store.max_run_bytes is too large for this platform".to_string(),
                    )
                })?)
            }
        }
    };
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
        max_run_bytes,
        auth_token,
        auth_token_env,
        basic_username,
        basic_password,
        basic_password_env,
        mtls_ca_pem,
        mtls_identity_pem,
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
            default_workers: 1,
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

    let default_workers = parse_u64_opt(task_tbl, "default_workers")?.unwrap_or(1);
    if default_workers == 0 {
        return Err(EffectsError::Log(
            "caps.toml: task.default_workers must be >= 1".to_string(),
        ));
    }

    Ok(TaskPolicy {
        default_workers,
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
    if let Some(replacement) = retired_high_level_op_replacement(op) {
        return Err(EffectsError::Log(format!(
            "caps.toml: legacy high-level op `{op}` is retired; use `{replacement}`"
        )));
    }
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
            if n <= 0 {
                None
            } else {
                Some(usize::try_from(n).map_err(|_| {
                    EffectsError::Log(format!(
                        "caps.toml: op {op} log_inline_max_bytes is too large for this platform"
                    ))
                })?)
            }
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
    use std::path::Path;

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
max_artifact_bytes_per_run = 456
"#,
        )
        .unwrap();
        assert_eq!(p.log.inline_max_bytes, Some(123));
        assert_eq!(p.log.max_artifact_bytes_per_run, Some(456));
        assert!(p.log.store_dir.is_some());
    }

    #[test]
    fn parses_store_run_budget() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["core/store::put"]

[store]
max_run_bytes = 2048
"#,
        )
        .unwrap();
        assert_eq!(p.store.max_run_bytes, Some(2048));
    }

    #[test]
    fn parses_store_auth_policy_fields() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["core/store::get"]

[store]
auth_token = "token-value"
auth_token_env = "GENESIS_TEST_TOKEN"
basic_username = "robot"
basic_password = "s3cr3t"
basic_password_env = "GENESIS_BASIC_PASS"
mtls_ca_pem = "./ca.pem"
mtls_identity_pem = "./id.pem"
"#,
        )
        .unwrap();
        assert_eq!(p.store.auth_token.as_deref(), Some("token-value"));
        assert_eq!(
            p.store.auth_token_env.as_deref(),
            Some("GENESIS_TEST_TOKEN")
        );
        assert_eq!(p.store.basic_username.as_deref(), Some("robot"));
        assert_eq!(p.store.basic_password.as_deref(), Some("s3cr3t"));
        assert_eq!(
            p.store.basic_password_env.as_deref(),
            Some("GENESIS_BASIC_PASS")
        );
        assert_eq!(p.store.mtls_ca_pem.as_deref(), Some(Path::new("./ca.pem")));
        assert_eq!(
            p.store.mtls_identity_pem.as_deref(),
            Some(Path::new("./id.pem"))
        );
    }

    #[test]
    fn load_resolves_relative_mtls_paths() {
        let td = tempfile::tempdir().unwrap();
        let caps = td.path().join("caps.toml");
        std::fs::write(
            &caps,
            r#"
allow = ["core/sync::pull"]

[store]
mtls_ca_pem = "./certs/ca.pem"
mtls_identity_pem = "./certs/id.pem"

[op."core/sync::pull"]
mtls_ca_pem = "./certs/op-ca.pem"
"#,
        )
        .unwrap();
        let p = CapsPolicy::load(&caps).unwrap();
        assert!(p.store.mtls_ca_pem.as_ref().unwrap().is_absolute());
        assert!(p.store.mtls_identity_pem.as_ref().unwrap().is_absolute());
        let op = p.op_policy("core/sync::pull").unwrap();
        assert!(
            op.extra
                .get("mtls_ca_pem")
                .and_then(|v| v.as_str())
                .is_some_and(|s| Path::new(s).is_absolute())
        );
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
default_workers = 3
max_tasks = 12
max_workers = 4
max_queue = 16
max_steps_per_task = 20
max_time_ms_per_task = 50
"#,
        )
        .unwrap();
        assert_eq!(p.task.default_workers, 3);
        assert_eq!(p.task.max_tasks, Some(12));
        assert_eq!(p.task.max_workers, Some(4));
        assert_eq!(p.task.max_queue, Some(16));
        assert_eq!(p.task.max_steps_per_task, Some(20));
        assert_eq!(p.task.max_time_ms_per_task, Some(50));
    }

    #[test]
    fn defaults_task_worker_budget_to_one_when_unspecified() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["core/task::await"]
"#,
        )
        .unwrap();
        assert_eq!(p.task.default_workers, 1);
    }

    #[test]
    fn rejects_zero_default_workers() {
        let err = CapsPolicy::from_toml_str(
            r#"
allow = ["core/task::await"]

[task]
default_workers = 0
"#,
        )
        .expect_err("must reject zero default workers");
        assert!(format!("{err}").contains("task.default_workers"));
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
    fn low_level_allow_does_not_authorize_high_level_alias() {
        let p = CapsPolicy::from_toml_str(
            r#"
allow = ["core/gc-low::pin"]

[op."core/gc-low::pin"]
timeout_ms = 10
"#,
        )
        .unwrap();
        assert!(!p.is_allowed("core/gc::pin"));
        assert!(p.op_policy("core/gc::pin").is_none());
    }

    #[test]
    fn rejects_legacy_high_level_ops_in_allow_and_op_tables() {
        let err = CapsPolicy::from_toml_str(
            r#"
allow = ["core/pkg::lock"]
"#,
        )
        .expect_err("must reject retired high-level op in allow list");
        assert!(format!("{err}").contains("legacy high-level op `core/pkg::lock`"));

        let err = CapsPolicy::from_toml_str(
            r#"
allow = ["core/pkg-low::lock"]

[op."core/pkg::lock"]
base_dir = "."
"#,
        )
        .expect_err("must reject retired high-level op in op table");
        assert!(format!("{err}").contains("legacy high-level op `core/pkg::lock`"));
    }
}
