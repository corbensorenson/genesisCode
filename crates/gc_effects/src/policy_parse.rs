use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::error::EffectsError;

use super::{LogPolicy, OpPolicy, RefsPolicy, RuntimePolicy, StorePolicy, TaskPolicy};

pub(super) fn retired_high_level_op_replacement(op: &str) -> Option<&'static str> {
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

pub(super) fn parse_log_policy(tbl: &toml::value::Table) -> Result<LogPolicy, EffectsError> {
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

pub(super) fn parse_store_policy(tbl: &toml::value::Table) -> Result<StorePolicy, EffectsError> {
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

pub(super) fn parse_refs_policy(tbl: &toml::value::Table) -> Result<RefsPolicy, EffectsError> {
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

pub(super) fn parse_task_policy(tbl: &toml::value::Table) -> Result<TaskPolicy, EffectsError> {
    let Some(v) = tbl.get("task") else {
        return Ok(TaskPolicy {
            default_workers: super::adaptive_default_task_workers(),
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

    let default_workers = parse_u64_opt(task_tbl, "default_workers")?
        .unwrap_or_else(super::adaptive_default_task_workers);
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

pub(super) fn parse_runtime_policy(
    tbl: &toml::value::Table,
) -> Result<RuntimePolicy, EffectsError> {
    let Some(v) = tbl.get("runtime") else {
        return Ok(RuntimePolicy {
            max_effect_ops: None,
            max_payload_bytes_per_op: None,
            max_payload_bytes_per_run: None,
            max_response_bytes_per_op: None,
            max_response_bytes_per_run: None,
        });
    };
    let runtime_tbl = v
        .as_table()
        .ok_or_else(|| EffectsError::Log("caps.toml: runtime must be a table".to_string()))?;

    fn parse_u64_opt(
        runtime_tbl: &toml::value::Table,
        key: &str,
    ) -> Result<Option<u64>, EffectsError> {
        match runtime_tbl.get(key) {
            None => Ok(None),
            Some(v) => {
                let n = v.as_integer().ok_or_else(|| {
                    EffectsError::Log(format!("caps.toml: runtime.{key} must be an integer"))
                })?;
                if n < 0 {
                    return Err(EffectsError::Log(format!(
                        "caps.toml: runtime.{key} must be >= 0"
                    )));
                }
                Ok(Some(n as u64))
            }
        }
    }

    fn parse_usize_opt(
        runtime_tbl: &toml::value::Table,
        key: &str,
    ) -> Result<Option<usize>, EffectsError> {
        let Some(raw) = parse_u64_opt(runtime_tbl, key)? else {
            return Ok(None);
        };
        Ok(Some(usize::try_from(raw).map_err(|_| {
            EffectsError::Log(format!(
                "caps.toml: runtime.{key} is too large for this platform"
            ))
        })?))
    }

    Ok(RuntimePolicy {
        max_effect_ops: parse_u64_opt(runtime_tbl, "max_effect_ops")?,
        max_payload_bytes_per_op: parse_usize_opt(runtime_tbl, "max_payload_bytes_per_op")?,
        max_payload_bytes_per_run: parse_usize_opt(runtime_tbl, "max_payload_bytes_per_run")?,
        max_response_bytes_per_op: parse_usize_opt(runtime_tbl, "max_response_bytes_per_op")?,
        max_response_bytes_per_run: parse_usize_opt(runtime_tbl, "max_response_bytes_per_run")?,
    })
}

pub(super) fn apply_op_cfg(
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
