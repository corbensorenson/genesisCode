use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::EffectsError;

#[path = "policy_parse.rs"]
mod policy_parse;
use policy_parse::{
    apply_op_cfg, parse_log_policy, parse_refs_policy, parse_runtime_policy, parse_store_policy,
    parse_task_policy, retired_high_level_op_replacement,
};

#[derive(Debug, Clone)]
pub struct CapsPolicy {
    ops: BTreeMap<String, OpPolicy>,
    pub log: LogPolicy,
    pub store: StorePolicy,
    pub refs: RefsPolicy,
    pub task: TaskPolicy,
    pub runtime: RuntimePolicy,
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
pub struct RuntimePolicy {
    pub max_effect_ops: Option<u64>,
    pub max_payload_bytes_per_op: Option<usize>,
    pub max_payload_bytes_per_run: Option<usize>,
    pub max_response_bytes_per_op: Option<usize>,
    pub max_response_bytes_per_run: Option<usize>,
}

fn adaptive_default_task_workers() -> u64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(1)
        .max(1)
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
                default_workers: adaptive_default_task_workers(),
                max_tasks: None,
                max_workers: None,
                max_queue: None,
                max_steps_per_task: None,
                max_time_ms_per_task: None,
            },
            runtime: RuntimePolicy {
                max_effect_ops: None,
                max_payload_bytes_per_op: None,
                max_payload_bytes_per_run: None,
                max_response_bytes_per_op: None,
                max_response_bytes_per_run: None,
            },
        }
    }

    pub fn is_allowed(&self, op: &str) -> bool {
        self.ops.contains_key(op)
    }

    pub fn op_policy(&self, op: &str) -> Option<&OpPolicy> {
        self.ops.get(op)
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
        let runtime = parse_runtime_policy(tbl)?;

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

        // Per-op configuration must use canonical [op."<op-symbol>"] tables.
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
                || k == "runtime"
            {
                continue;
            }
            if v.is_table() {
                return Err(EffectsError::Log(format!(
                    "caps.toml: top-level table `{k}` is not supported; define per-op policy under [op.\"{k}\"]"
                )));
            }
            return Err(EffectsError::Log(format!(
                "caps.toml: unknown top-level key `{k}`"
            )));
        }

        Ok(Self {
            ops,
            log,
            store,
            refs,
            task,
            runtime,
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

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
