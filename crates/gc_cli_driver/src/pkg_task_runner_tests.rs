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
