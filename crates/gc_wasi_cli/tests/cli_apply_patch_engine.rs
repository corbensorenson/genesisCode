use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cargo_bin_cmd!("genesis_wasi")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();
    artifact
}

fn copy_pkg_basic_fixture(dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
        std::fs::copy(fixture.join(name), dst.join(name)).unwrap();
    }
}

fn run_json(current_dir: &std::path::Path, args: &[&str]) -> JsonValue {
    let out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(current_dir)
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

fn assert_artifact_fields_equal(lhs: &JsonValue, rhs: &JsonValue) {
    let lhs_data = lhs.get("data").expect("lhs data");
    let rhs_data = rhs.get("data").expect("rhs data");
    for key in [
        "patch_artifact",
        "report_artifact",
        "acceptance_artifact",
        "package_artifact",
    ] {
        assert_eq!(
            lhs_data.get(key).and_then(JsonValue::as_str),
            rhs_data.get(key).and_then(JsonValue::as_str),
            "artifact mismatch for {key}"
        );
    }
}

#[test]
fn apply_patch_selfhost_artifact_matches_default_frontend_artifacts() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let default_dir = td.path().join("pkg_default");
    let artifact_dir = td.path().join("pkg_artifact");
    copy_pkg_basic_fixture(&default_dir);
    copy_pkg_basic_fixture(&artifact_dir);

    let default_v = run_json(
        &default_dir,
        &[
            "--json",
            "apply-patch",
            "pure.gcpatch",
            "--pkg",
            "package.toml",
        ],
    );
    let artifact_v = run_json(
        &artifact_dir,
        &[
            "--json",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "apply-patch",
            "pure.gcpatch",
            "--pkg",
            "package.toml",
        ],
    );

    assert_artifact_fields_equal(&default_v, &artifact_v);
}

#[test]
fn apply_patch_selfhost_only_matches_default_frontend_artifacts() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let default_dir = td.path().join("pkg_default");
    let strict_dir = td.path().join("pkg_strict");
    copy_pkg_basic_fixture(&default_dir);
    copy_pkg_basic_fixture(&strict_dir);

    let default_v = run_json(
        &default_dir,
        &[
            "--json",
            "apply-patch",
            "pure.gcpatch",
            "--pkg",
            "package.toml",
        ],
    );
    let strict_v = run_json(
        &strict_dir,
        &[
            "--json",
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "apply-patch",
            "pure.gcpatch",
            "--pkg",
            "package.toml",
        ],
    );

    assert_artifact_fields_equal(&default_v, &strict_v);
}
