use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

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
fn test_json_output_is_valid_and_exit_code_is_stable() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_basic");
    let dst = td.path().join("pkg_basic");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let caps = dst.join("caps.toml");

    let out = cargo_bin_cmd!("genesis")
        .args(["--json", "test", "--pkg"])
        .arg(&pkg)
        .args(["--caps"])
        .arg(&caps)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/test-v0.2")
    );
    let acc = v
        .get("data")
        .and_then(|d| d.get("acceptance_artifact"))
        .and_then(|x| x.as_str())
        .expect("acceptance_artifact");
    assert_eq!(acc.len(), 64);
}

#[test]
fn run_denied_capability_has_exit_code_41() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let prog = dir.join("prog.gc");
    fs::write(
        &prog,
        r#"
          (def prog
            (core/effect::perform
              'sys/time::now
              {}
              (fn (t)
                (core/effect::pure t))))
          prog
        "#,
    )
    .unwrap();

    let caps = dir.join("caps.toml");
    fs::write(&caps, r#"allow = []"#).unwrap();

    let log = dir.join("out.gclog");

    cargo_bin_cmd!("genesis")
        .args(["run"])
        .arg(&prog)
        .args(["--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log)
        .assert()
        .failure()
        .code(41);

    assert!(log.exists(), "run should still emit a deterministic log");
}

#[test]
fn fmt_check_failure_has_exit_code_11() {
    let td = tempfile::tempdir().unwrap();
    let p = td.path().join("x.gc");
    fs::write(
        &p,
        // not canonical: multi-arg app should be reprinted nested
        r#"
          ((fn (x y) (prim int/add x y)) 1 2)
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args(["fmt", "--check"])
        .arg(&p)
        .assert()
        .failure()
        .code(11);
}

