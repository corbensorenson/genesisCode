use std::path::{Path, PathBuf};

use gc_pkg::WorkspaceConfig;

pub(crate) enum WorkspaceTaskAction {
    Test {
        pkg: PathBuf,
        caps: Option<PathBuf>,
    },
    Pack {
        pkg: PathBuf,
    },
    Typecheck {
        pkg: PathBuf,
    },
    Run {
        file: PathBuf,
        caps: Option<PathBuf>,
        log: Option<PathBuf>,
        engine: Option<String>,
    },
    Contract {
        file: PathBuf,
        caps: Option<PathBuf>,
        log: Option<PathBuf>,
        engine: Option<String>,
        contract_hash_hex: String,
    },
    Eval {
        file: PathBuf,
        engine: Option<String>,
        stage1_pipeline: bool,
        stage1_gate: bool,
        stage2_gate: bool,
    },
    Fmt {
        file: PathBuf,
        check: bool,
        engine: Option<String>,
    },
    Optimize {
        file: PathBuf,
        out: Option<PathBuf>,
        emit_wasm: Option<PathBuf>,
        engine: Option<String>,
        stage1_gate: bool,
        stage2_gate: bool,
    },
}

pub(crate) fn resolve_workspace_task(
    workspace_file: &Path,
    task_name: &str,
) -> Result<WorkspaceTaskAction, String> {
    let ws = WorkspaceConfig::load(workspace_file).map_err(|e| e.to_string())?;
    let task = ws.tasks.get(task_name).ok_or_else(|| {
        format!(
            "task `{task_name}` not found in {}",
            workspace_file.display()
        )
    })?;

    let cmd = task.cmd.trim().to_ascii_lowercase();
    match cmd.as_str() {
        "test" => Ok(WorkspaceTaskAction::Test {
            pkg: resolve_pkg_path(workspace_file, task),
            caps: parse_task_caps(workspace_file, task_name, &task.args)?,
        }),
        "pack" | "build" => Ok(WorkspaceTaskAction::Pack {
            pkg: resolve_pkg_path(workspace_file, task),
        }),
        "typecheck" | "lint" => Ok(WorkspaceTaskAction::Typecheck {
            pkg: resolve_pkg_path(workspace_file, task),
        }),
        "run" | "bench" => {
            let args = parse_run_like_args(workspace_file, task_name, &task.args)?;
            Ok(WorkspaceTaskAction::Run {
                file: resolve_file_path(workspace_file, task_name, task)?,
                caps: args.caps,
                log: args.log,
                engine: args.engine,
            })
        }
        "contract" => {
            let args = parse_contract_task_args(workspace_file, task_name, &task.args)?;
            Ok(WorkspaceTaskAction::Contract {
                file: resolve_file_path(workspace_file, task_name, task)?,
                caps: args.caps,
                log: args.log,
                engine: args.engine,
                contract_hash_hex: args.contract_hash_hex,
            })
        }
        "eval" => {
            let args = parse_eval_args(task_name, &task.args)?;
            Ok(WorkspaceTaskAction::Eval {
                file: resolve_file_path(workspace_file, task_name, task)?,
                engine: args.engine,
                stage1_pipeline: args.stage1_pipeline,
                stage1_gate: args.stage1_gate,
                stage2_gate: args.stage2_gate,
            })
        }
        "fmt" => {
            let args = parse_fmt_args(task_name, &task.args)?;
            Ok(WorkspaceTaskAction::Fmt {
                file: resolve_file_path(workspace_file, task_name, task)?,
                check: args.check,
                engine: args.engine,
            })
        }
        "optimize" => {
            let args = parse_optimize_args(workspace_file, task_name, &task.args)?;
            Ok(WorkspaceTaskAction::Optimize {
                file: resolve_file_path(workspace_file, task_name, task)?,
                out: args.out,
                emit_wasm: args.emit_wasm,
                engine: args.engine,
                stage1_gate: args.stage1_gate,
                stage2_gate: args.stage2_gate,
            })
        }
        other => Err(format!(
            "unsupported task cmd `{other}` for task `{task_name}`; supported: \
test|pack|build|typecheck|lint|run|bench|contract|eval|fmt|optimize"
        )),
    }
}

fn resolve_pkg_path(workspace_file: &Path, task: &gc_pkg::WorkspaceTask) -> PathBuf {
    let raw = task
        .pkg
        .as_deref()
        .or(task.file.as_deref())
        .unwrap_or("package.toml");
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        return candidate;
    }
    workspace_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(candidate)
}

fn resolve_file_path(
    workspace_file: &Path,
    task_name: &str,
    task: &gc_pkg::WorkspaceTask,
) -> Result<PathBuf, String> {
    let raw = task
        .file
        .as_deref()
        .or(task.pkg.as_deref())
        .ok_or_else(|| {
            format!("task `{task_name}` requires `file = \"...\"` (or `pkg = \"...\"`)")
        })?;
    Ok(resolve_workspace_relative_path(workspace_file, raw))
}

fn resolve_workspace_relative_path(workspace_file: &Path, raw: &str) -> PathBuf {
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        return candidate;
    }
    workspace_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(candidate)
}

fn parse_task_caps(
    workspace_file: &Path,
    task_name: &str,
    args: &[String],
) -> Result<Option<PathBuf>, String> {
    let mut caps: Option<PathBuf> = None;
    let mut i = 0_usize;
    while i < args.len() {
        match args[i].as_str() {
            "--caps" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --caps"));
                };
                caps = Some(resolve_workspace_relative_path(workspace_file, raw));
                i += 2;
            }
            other => {
                return Err(format!(
                    "task `{task_name}` has unsupported arg `{other}` for cmd=test; supported: --caps <path>"
                ));
            }
        }
    }
    Ok(caps)
}

fn parse_run_like_args(
    workspace_file: &Path,
    task_name: &str,
    args: &[String],
) -> Result<RunLikeTaskArgs, String> {
    let mut caps: Option<PathBuf> = None;
    let mut log: Option<PathBuf> = None;
    let mut engine: Option<String> = None;
    let mut i = 0_usize;
    while i < args.len() {
        match args[i].as_str() {
            "--caps" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --caps"));
                };
                caps = Some(resolve_workspace_relative_path(workspace_file, raw));
                i += 2;
            }
            "--log" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --log"));
                };
                log = Some(resolve_workspace_relative_path(workspace_file, raw));
                i += 2;
            }
            "--engine" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --engine"));
                };
                engine = Some(raw.trim().to_ascii_lowercase());
                i += 2;
            }
            other => {
                return Err(format!(
                    "task `{task_name}` has unsupported arg `{other}` for cmd=run|bench; \
supported: --caps <path>, --log <path>, --engine <selfhost|rust>"
                ));
            }
        }
    }
    Ok(RunLikeTaskArgs { caps, log, engine })
}

#[derive(Default)]
struct RunLikeTaskArgs {
    caps: Option<PathBuf>,
    log: Option<PathBuf>,
    engine: Option<String>,
}

#[derive(Default)]
struct ContractTaskArgs {
    caps: Option<PathBuf>,
    log: Option<PathBuf>,
    engine: Option<String>,
    contract_hash_hex: String,
}

fn parse_contract_task_args(
    workspace_file: &Path,
    task_name: &str,
    args: &[String],
) -> Result<ContractTaskArgs, String> {
    let mut out = ContractTaskArgs::default();
    let mut i = 0_usize;
    while i < args.len() {
        match args[i].as_str() {
            "--caps" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --caps"));
                };
                out.caps = Some(resolve_workspace_relative_path(workspace_file, raw));
                i += 2;
            }
            "--log" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --log"));
                };
                out.log = Some(resolve_workspace_relative_path(workspace_file, raw));
                i += 2;
            }
            "--engine" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --engine"));
                };
                out.engine = Some(raw.trim().to_ascii_lowercase());
                i += 2;
            }
            "--contract-h" | "--contract-hash" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --contract-h"));
                };
                out.contract_hash_hex = parse_contract_hash_hex(task_name, raw)?;
                i += 2;
            }
            other => {
                return Err(format!(
                    "task `{task_name}` has unsupported arg `{other}` for cmd=contract; supported: \
--contract-h <hex64>, --caps <path>, --log <path>, --engine <selfhost|rust>"
                ));
            }
        }
    }
    if out.contract_hash_hex.is_empty() {
        return Err(format!(
            "task `{task_name}` missing required --contract-h <hex64> for cmd=contract"
        ));
    }
    Ok(out)
}

fn parse_contract_hash_hex(task_name: &str, raw: &str) -> Result<String, String> {
    let h = raw.trim().to_ascii_lowercase();
    if h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "task `{task_name}` has invalid --contract-h `{raw}` (expected 64 hex chars)"
        ));
    }
    Ok(h)
}

pub(crate) fn verify_contract_task_file_hash(
    file: &Path,
    expected_hash_hex: &str,
) -> Result<String, String> {
    let bytes = std::fs::read(file)
        .map_err(|e| format!("read contract task file {}: {e}", file.display()))?;
    let actual = blake3::hash(&bytes).to_hex().to_string();
    if actual != expected_hash_hex {
        return Err(format!(
            "contract task hash mismatch for {}: expected {}, got {}",
            file.display(),
            expected_hash_hex,
            actual
        ));
    }
    Ok(actual)
}

#[derive(Default)]
struct EvalTaskArgs {
    engine: Option<String>,
    stage1_pipeline: bool,
    stage1_gate: bool,
    stage2_gate: bool,
}

fn parse_eval_args(task_name: &str, args: &[String]) -> Result<EvalTaskArgs, String> {
    let mut out = EvalTaskArgs::default();
    let mut i = 0_usize;
    while i < args.len() {
        match args[i].as_str() {
            "--engine" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --engine"));
                };
                out.engine = Some(raw.trim().to_ascii_lowercase());
                i += 2;
            }
            "--stage1-pipeline" => {
                out.stage1_pipeline = true;
                i += 1;
            }
            "--stage1-gate" => {
                out.stage1_gate = true;
                i += 1;
            }
            "--stage2-gate" => {
                out.stage2_gate = true;
                i += 1;
            }
            other => {
                return Err(format!(
                    "task `{task_name}` has unsupported arg `{other}` for cmd=eval; supported: \
--engine <selfhost|rust>, --stage1-pipeline, --stage1-gate, --stage2-gate"
                ));
            }
        }
    }
    Ok(out)
}

#[derive(Default)]
struct FmtTaskArgs {
    check: bool,
    engine: Option<String>,
}

fn parse_fmt_args(task_name: &str, args: &[String]) -> Result<FmtTaskArgs, String> {
    let mut out = FmtTaskArgs::default();
    let mut i = 0_usize;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => {
                out.check = true;
                i += 1;
            }
            "--engine" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --engine"));
                };
                out.engine = Some(raw.trim().to_ascii_lowercase());
                i += 2;
            }
            other => {
                return Err(format!(
                    "task `{task_name}` has unsupported arg `{other}` for cmd=fmt; supported: \
--check, --engine <selfhost|rust>"
                ));
            }
        }
    }
    Ok(out)
}

#[derive(Default)]
struct OptimizeTaskArgs {
    out: Option<PathBuf>,
    emit_wasm: Option<PathBuf>,
    engine: Option<String>,
    stage1_gate: bool,
    stage2_gate: bool,
}

fn parse_optimize_args(
    workspace_file: &Path,
    task_name: &str,
    args: &[String],
) -> Result<OptimizeTaskArgs, String> {
    let mut out = OptimizeTaskArgs::default();
    let mut i = 0_usize;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --out"));
                };
                out.out = Some(resolve_workspace_relative_path(workspace_file, raw));
                i += 2;
            }
            "--emit-wasm" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --emit-wasm"));
                };
                out.emit_wasm = Some(resolve_workspace_relative_path(workspace_file, raw));
                i += 2;
            }
            "--engine" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(format!("task `{task_name}` missing value for --engine"));
                };
                out.engine = Some(raw.trim().to_ascii_lowercase());
                i += 2;
            }
            "--stage1-gate" => {
                out.stage1_gate = true;
                i += 1;
            }
            "--stage2-gate" => {
                out.stage2_gate = true;
                i += 1;
            }
            other => {
                return Err(format!(
                    "task `{task_name}` has unsupported arg `{other}` for cmd=optimize; supported: \
--out <path>, --emit-wasm <path>, --engine <selfhost|rust>, --stage1-gate, --stage2-gate"
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceTaskAction, resolve_workspace_task, verify_contract_task_file_hash};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_tmp_dir() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "gcpm-task-runner-{stamp}-{}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_workspace(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
    }

    #[test]
    fn resolves_alias_tasks_build_and_lint() {
        let dir = unique_tmp_dir();
        let workspace = dir.join("genesis.workspace.toml");
        write_workspace(
            &workspace,
            r#"
version = 1
workspace = "ws"

[[members]]
name = "root"
path = "."
role = "root"

[tasks."build-local"]
cmd = "build"
pkg = "package.toml"

[tasks."lint-local"]
cmd = "lint"
pkg = "package.toml"
"#,
        );

        match resolve_workspace_task(&workspace, "build-local").unwrap() {
            WorkspaceTaskAction::Pack { pkg } => assert!(pkg.ends_with("package.toml")),
            _ => panic!("build alias must resolve to pack"),
        }
        match resolve_workspace_task(&workspace, "lint-local").unwrap() {
            WorkspaceTaskAction::Typecheck { pkg } => assert!(pkg.ends_with("package.toml")),
            _ => panic!("lint alias must resolve to typecheck"),
        }
    }

    #[test]
    fn resolves_run_eval_fmt_optimize_with_task_args() {
        let dir = unique_tmp_dir();
        let workspace = dir.join("genesis.workspace.toml");
        write_workspace(
            &workspace,
            r#"
version = 1
workspace = "ws"

[[members]]
name = "root"
path = "."
role = "root"

[tasks."run-local"]
cmd = "run"
file = "workflow.gc"
args = ["--caps", "caps.toml", "--log", ".genesis/logs/run.gclog", "--engine", "selfhost"]

[tasks."eval-local"]
cmd = "eval"
file = "lib.gc"
args = ["--stage1-pipeline", "--stage2-gate"]

[tasks."fmt-local"]
cmd = "fmt"
file = "lib.gc"
args = ["--check", "--engine", "selfhost"]

[tasks."opt-local"]
cmd = "optimize"
file = "lib.gc"
args = ["--out", "opt.gc", "--emit-wasm", "opt.wasm", "--stage1-gate"]
"#,
        );

        match resolve_workspace_task(&workspace, "run-local").unwrap() {
            WorkspaceTaskAction::Run {
                file,
                caps,
                log,
                engine,
            } => {
                assert!(file.ends_with("workflow.gc"));
                assert!(caps.unwrap().ends_with("caps.toml"));
                assert!(log.unwrap().ends_with(".genesis/logs/run.gclog"));
                assert_eq!(engine.as_deref(), Some("selfhost"));
            }
            _ => panic!("run task must resolve to run action"),
        }

        match resolve_workspace_task(&workspace, "eval-local").unwrap() {
            WorkspaceTaskAction::Eval {
                stage1_pipeline,
                stage2_gate,
                ..
            } => {
                assert!(stage1_pipeline);
                assert!(stage2_gate);
            }
            _ => panic!("eval task must resolve to eval action"),
        }

        match resolve_workspace_task(&workspace, "fmt-local").unwrap() {
            WorkspaceTaskAction::Fmt { check, engine, .. } => {
                assert!(check);
                assert_eq!(engine.as_deref(), Some("selfhost"));
            }
            _ => panic!("fmt task must resolve to fmt action"),
        }

        match resolve_workspace_task(&workspace, "opt-local").unwrap() {
            WorkspaceTaskAction::Optimize {
                out,
                emit_wasm,
                stage1_gate,
                ..
            } => {
                assert!(out.unwrap().ends_with("opt.gc"));
                assert!(emit_wasm.unwrap().ends_with("opt.wasm"));
                assert!(stage1_gate);
            }
            _ => panic!("optimize task must resolve to optimize action"),
        }
    }

    #[test]
    fn resolves_contract_task_with_hash_pin_and_validates_file_hash() {
        let dir = unique_tmp_dir();
        let workspace = dir.join("genesis.workspace.toml");
        let contract_file = dir.join("contract_task.gc");
        fs::write(
            &contract_file,
            "(def task/prog (core/effect::pure {:ok true}))\n",
        )
        .unwrap();
        let contract_h = blake3::hash(&fs::read(&contract_file).unwrap())
            .to_hex()
            .to_string();

        write_workspace(
            &workspace,
            &format!(
                r#"
version = 1
workspace = "ws"

[[members]]
name = "root"
path = "."
role = "root"

[tasks."contract-local"]
cmd = "contract"
file = "contract_task.gc"
args = ["--contract-h", "{contract_h}", "--caps", "caps.toml", "--log", ".genesis/logs/contract.gclog", "--engine", "selfhost"]
"#
            ),
        );

        match resolve_workspace_task(&workspace, "contract-local").unwrap() {
            WorkspaceTaskAction::Contract {
                file,
                caps,
                log,
                engine,
                contract_hash_hex,
            } => {
                assert!(file.ends_with("contract_task.gc"));
                assert!(caps.unwrap().ends_with("caps.toml"));
                assert!(log.unwrap().ends_with(".genesis/logs/contract.gclog"));
                assert_eq!(engine.as_deref(), Some("selfhost"));
                assert_eq!(contract_hash_hex, contract_h);
                let verified = verify_contract_task_file_hash(&file, &contract_hash_hex).unwrap();
                assert_eq!(verified, contract_h);
            }
            _ => panic!("contract task must resolve to contract action"),
        }
    }

    #[test]
    fn contract_task_requires_hash_pin_and_reports_mismatch() {
        let dir = unique_tmp_dir();
        let workspace = dir.join("genesis.workspace.toml");
        let contract_file = dir.join("contract_task.gc");
        fs::write(
            &contract_file,
            "(def task/prog (core/effect::pure {:ok true}))\n",
        )
        .unwrap();

        write_workspace(
            &workspace,
            r#"
version = 1
workspace = "ws"

[[members]]
name = "root"
path = "."
role = "root"

[tasks."contract-missing-hash"]
cmd = "contract"
file = "contract_task.gc"
args = ["--caps", "caps.toml"]
"#,
        );
        let err = match resolve_workspace_task(&workspace, "contract-missing-hash") {
            Ok(_) => panic!("expected missing-hash contract task to fail"),
            Err(e) => e,
        };
        assert!(err.contains("missing required --contract-h"));

        write_workspace(
            &workspace,
            r#"
version = 1
workspace = "ws"

[[members]]
name = "root"
path = "."
role = "root"

[tasks."contract-bad-hash"]
cmd = "contract"
file = "contract_task.gc"
args = ["--contract-h", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
"#,
        );
        let action = resolve_workspace_task(&workspace, "contract-bad-hash").unwrap();
        let WorkspaceTaskAction::Contract {
            file,
            contract_hash_hex,
            ..
        } = action
        else {
            panic!("expected contract action");
        };
        let mismatch = verify_contract_task_file_hash(&file, &contract_hash_hex).unwrap_err();
        assert!(mismatch.contains("contract task hash mismatch"));
    }
}
