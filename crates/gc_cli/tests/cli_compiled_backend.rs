use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::tempdir;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis");
    c.env("GENESIS_ALLOW_RUST_ENGINE", "1");
    c
}

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/spec")
        .join(path)
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        if from.file_name().is_some_and(|n| n == ".genesis") {
            continue;
        }
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to)?;
        } else if ty.is_file() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[test]
fn eval_defaults_to_compiled_backend() {
    let td = tempdir().unwrap();
    let file = td.path().join("eval.gc");
    fs::write(&file, "(def demo/x 41)\n(prim int/add demo/x 1)\n").unwrap();

    let out = cmd()
        .args(["--json", "eval", file.to_str().unwrap(), "--engine", "rust"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("json");
    assert_eq!(
        v.get("data")
            .and_then(|d| d.get("kernel_eval_backend"))
            .and_then(|x| x.as_str()),
        Some("compiled")
    );
}

#[test]
fn eval_can_force_tree_walk_backend_for_parity_guard() {
    let td = tempdir().unwrap();
    let file = td.path().join("eval.gc");
    fs::write(&file, "(def demo/x 41)\n(prim int/add demo/x 1)\n").unwrap();

    let out = cmd()
        .env("GENESIS_DISABLE_COMPILED_EVAL", "1")
        .args(["--json", "eval", file.to_str().unwrap(), "--engine", "rust"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("json");
    assert_eq!(
        v.get("data")
            .and_then(|d| d.get("kernel_eval_backend"))
            .and_then(|x| x.as_str()),
        Some("tree-walk")
    );
}

#[test]
fn run_replay_and_test_include_compiled_backend_metadata() {
    let td = tempdir().unwrap();
    let file = td.path().join("prog.gc");
    let caps = td.path().join("caps.toml");
    let log = td.path().join("prog.gclog");
    fs::write(&file, "(def prog (core/effect::pure 7))\nprog\n").unwrap();
    fs::write(&caps, "allow = []\n").unwrap();

    let run_out = cmd()
        .args([
            "--json",
            "run",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--caps",
            caps.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let run_v: serde_json::Value = serde_json::from_slice(&run_out).expect("json");
    assert_eq!(
        run_v
            .get("data")
            .and_then(|d| d.get("kernel_eval_backend"))
            .and_then(|x| x.as_str()),
        Some("compiled")
    );

    let replay_out = cmd()
        .args([
            "--json",
            "replay",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let replay_v: serde_json::Value = serde_json::from_slice(&replay_out).expect("json");
    assert_eq!(
        replay_v
            .get("data")
            .and_then(|d| d.get("kernel_eval_backend"))
            .and_then(|x| x.as_str()),
        Some("compiled")
    );

    let src = fixture("pkg_basic");
    let dst = td.path().join("pkg_basic");
    copy_dir_all(&src, &dst).unwrap();
    let pkg = dst.join("package.toml");
    let test_caps = dst.join("caps.toml");
    let test_out = cmd()
        .args(["--json", "test", "--pkg"])
        .arg(&pkg)
        .args(["--caps"])
        .arg(&test_caps)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let test_v: serde_json::Value = serde_json::from_slice(&test_out).expect("json");
    assert_eq!(
        test_v
            .get("data")
            .and_then(|d| d.get("kernel_eval_backend_default"))
            .and_then(|x| x.as_str()),
        Some("compiled-with-treewalk-fallback")
    );
}
