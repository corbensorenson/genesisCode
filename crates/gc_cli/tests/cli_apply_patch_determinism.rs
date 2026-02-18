use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

mod support;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

fn copy_pkg_basic_fixture(dst: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    std::fs::create_dir_all(dst).unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
        std::fs::copy(fixture.join(name), dst.join(name)).unwrap();
    }
    (dst.join("package.toml"), dst.join("pure.gcpatch"))
}

fn run_apply_patch_json(
    artifact: &std::path::Path,
    patch: &std::path::Path,
    pkg: &std::path::Path,
) -> JsonValue {
    let out = cargo_bin_cmd!("genesis")
        .args([
            "--json",
            "--coreform-frontend",
            "selfhost",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "apply-patch",
            patch.to_str().unwrap(),
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn apply_patch_is_deterministic_across_reruns_under_selfhost_frontend() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());

    let (pkg1, patch1) = copy_pkg_basic_fixture(&td.path().join("pkg1"));
    let (pkg2, patch2) = copy_pkg_basic_fixture(&td.path().join("pkg2"));

    let v1 = run_apply_patch_json(&artifact, &patch1, &pkg1);
    let v2 = run_apply_patch_json(&artifact, &patch2, &pkg2);

    for key in [
        "patch_artifact",
        "report_artifact",
        "acceptance_artifact",
        "package_artifact",
    ] {
        assert_eq!(
            v1["data"].get(key).and_then(JsonValue::as_str),
            v2["data"].get(key).and_then(JsonValue::as_str),
            "apply-patch output must be deterministic for {key}"
        );
    }
}
