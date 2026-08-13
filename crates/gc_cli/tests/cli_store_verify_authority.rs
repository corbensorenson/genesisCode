use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn copy_toolchain_artifact(dir: &Path) -> PathBuf {
    let source = std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("selfhost/toolchain.gc")
        });
    let destination = dir.join("selfhost_toolchain.gc");
    fs::copy(source, &destination).expect("copy selfhost toolchain artifact");
    destination
}

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"allow = ["core/store::put", "core/store::verify"]

[store]
dir = "./.genesis/store"
"#,
    )
    .expect("write caps");
    caps
}

fn command(dir: &Path, artifact: &Path, caps: &Path) -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("genesis");
    command
        .current_dir(dir)
        .arg("--selfhost-artifact")
        .arg(artifact)
        .args(["store", "--caps"])
        .arg(caps);
    command
}

#[test]
fn production_store_verify_supports_specific_and_filtered_scan_modes() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let dir = temporary.path();
    let artifact = copy_toolchain_artifact(dir);
    let caps = write_caps(dir);
    let input = dir.join("artifact.gc");
    fs::write(&input, "{:k \"v\"}\n").expect("write input");

    let output = command(dir, &artifact, &caps)
        .args(["put", "--input"])
        .arg(&input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let hash = String::from_utf8(output).expect("UTF-8 hash");
    let hash = hash.trim();

    command(dir, &artifact, &caps)
        .arg("verify")
        .arg(hash)
        .assert()
        .success()
        .stdout("ok 1\n");

    let store = dir.join(".genesis/store");
    fs::write(store.join(".tmp-uncommitted"), b"ignored").expect("write temporary file");
    fs::create_dir(store.join("f".repeat(64))).expect("create hash-shaped directory");
    command(dir, &artifact, &caps)
        .arg("verify")
        .assert()
        .success()
        .stdout("ok 1\n");
}

#[test]
fn production_store_verify_reports_authoritative_corruption_code() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let dir = temporary.path();
    let artifact = copy_toolchain_artifact(dir);
    let caps = write_caps(dir);
    let input = dir.join("artifact.gc");
    fs::write(&input, "{:k 1}\n").expect("write input");

    let output = command(dir, &artifact, &caps)
        .args(["put", "--input"])
        .arg(&input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let hash = String::from_utf8(output).expect("UTF-8 hash");
    let hash = hash.trim();
    fs::write(dir.join(".genesis/store").join(hash), b"corrupted").expect("corrupt artifact");

    command(dir, &artifact, &caps)
        .arg("verify")
        .arg(hash)
        .assert()
        .code(50)
        .stdout(predicate::str::contains("core/store/corruption"))
        .stderr(predicate::str::contains("core/store/corruption"));
}
