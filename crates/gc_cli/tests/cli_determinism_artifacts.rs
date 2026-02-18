use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

mod support;

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

fn fixture_pkg_basic(dst: &Path) -> PathBuf {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/spec/pkg_basic");
    copy_dir_all(&src, dst).unwrap();
    dst.join("package.toml")
}

fn run_hash_stdout(cmd: &mut assert_cmd::Command) -> String {
    let out = cmd
        .assert()
        .success()
        .stdout(predicate::str::is_match("[0-9a-f]{64}\\s*").unwrap())
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
}

#[test]
fn pack_is_deterministic_across_reruns_under_selfhost_frontend() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path().join("pkg");
    let pkg = fixture_pkg_basic(&dir);

    let artifact = support::copy_repo_toolchain_artifact(&dir);

    let mut c1 = cargo_bin_cmd!("genesis");
    c1.env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .current_dir(&dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "pack",
            "--pkg",
            pkg.to_str().unwrap(),
        ]);
    let h1 = run_hash_stdout(&mut c1);

    let mut c2 = cargo_bin_cmd!("genesis");
    c2.env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .current_dir(&dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "pack",
            "--pkg",
            pkg.to_str().unwrap(),
        ]);
    let h2 = run_hash_stdout(&mut c2);
    assert_eq!(h1, h2, "pack output hash must be deterministic");
}

#[test]
fn test_acceptance_is_deterministic_across_reruns_under_selfhost_frontend() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path().join("pkg");
    let pkg = fixture_pkg_basic(&dir);

    let artifact = support::copy_repo_toolchain_artifact(&dir);
    let caps = dir.join("caps.toml");

    let mut c1 = cargo_bin_cmd!("genesis");
    c1.env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .current_dir(&dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "test",
            "--pkg",
            pkg.to_str().unwrap(),
            "--caps",
            caps.to_str().unwrap(),
        ]);
    let h1 = run_hash_stdout(&mut c1);

    let mut c2 = cargo_bin_cmd!("genesis");
    c2.env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .current_dir(&dir)
        .args([
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "test",
            "--pkg",
            pkg.to_str().unwrap(),
            "--caps",
            caps.to_str().unwrap(),
        ]);
    let h2 = run_hash_stdout(&mut c2);
    assert_eq!(h1, h2, "test acceptance hash must be deterministic");
}
