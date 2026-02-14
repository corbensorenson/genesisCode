use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

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

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/spec")
        .join(path)
}

#[test]
fn fmt_check_is_idempotent_on_fixture() {
    let td = tempfile::tempdir().unwrap();
    let inp = fixture("pkg_basic/basic.gc");
    let out = td.path().join("basic.gc");
    fs::copy(&inp, &out).unwrap();

    // Fixture sources aren't required to be canonical; ensure `fmt` makes them canonical
    // and `fmt --check` is idempotent after that.
    cargo_bin_cmd!("genesis")
        .args(["fmt"])
        .arg(&out)
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(["fmt", "--check"])
        .arg(&out)
        .assert()
        .success();
}

#[test]
fn test_pkg_basic_obligations_succeed() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_basic");
    let dst = td.path().join("pkg_basic");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let caps = dst.join("caps.toml");

    cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .args(["--caps"])
        .arg(&caps)
        .assert()
        .success()
        .stdout(predicate::str::is_match("[0-9a-f]{64}\\s*").unwrap());
}

#[test]
fn pack_is_stable_independent_of_invocation_path() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_basic");
    let dst = td.path().join("pkg_basic");
    copy_dir_all(&src, &dst).unwrap();

    let pkg_abs = dst.join("package.toml");

    let out_abs = cargo_bin_cmd!("genesis")
        .args(["pack", "--pkg"])
        .arg(&pkg_abs)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let h_abs = String::from_utf8(out_abs).unwrap();
    let h_abs = h_abs.trim().to_string();

    let out_rel = cargo_bin_cmd!("genesis")
        .current_dir(&dst)
        .args(["pack", "--pkg"])
        .arg("package.toml")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let h_rel = String::from_utf8(out_rel).unwrap();
    let h_rel = h_rel.trim().to_string();

    assert_eq!(h_abs, h_rel);
}

#[test]
fn run_and_replay_roundtrip_effect_program() {
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
    fs::write(&caps, r#"allow = ["sys/time::now"]"#).unwrap();

    let log = dir.join("out.gclog");

    let run_out = cargo_bin_cmd!("genesis")
        .args(["run"])
        .arg(&prog)
        .args(["--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let replay_out = cargo_bin_cmd!("genesis")
        .args(["replay"])
        .arg(&prog)
        .args(["--log"])
        .arg(&log)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let run_s = String::from_utf8(run_out).unwrap();
    let replay_s = String::from_utf8(replay_out).unwrap();
    assert_eq!(run_s.trim(), replay_s.trim());
}
