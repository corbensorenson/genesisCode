use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

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

fn parse_hash_field<'a>(data: &'a JsonValue, key: &str) -> &'a str {
    let h = data
        .get(key)
        .and_then(JsonValue::as_str)
        .unwrap_or_else(|| panic!("missing {key}"));
    assert_eq!(h.len(), 64, "{key} must be 64-hex hash");
    h
}

#[test]
fn apply_patch_json_schema_and_artifacts_are_emitted() {
    let td = tempdir().unwrap();
    let pkg_dir = td.path().join("pkg");
    copy_pkg_basic_fixture(&pkg_dir);

    let out = cargo_bin_cmd!("genesis_wasi")
        .current_dir(&pkg_dir)
        .args([
            "--json",
            "apply-patch",
            "pure.gcpatch",
            "--pkg",
            "package.toml",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(JsonValue::as_str),
        Some("genesis/apply-patch-v0.2"),
        "{v}"
    );
    assert_eq!(v.get("ok").and_then(JsonValue::as_bool), Some(true), "{v}");

    let data = v.get("data").expect("data");
    let patch_h = parse_hash_field(data, "patch_artifact");
    let report_h = parse_hash_field(data, "report_artifact");
    let acceptance_h = parse_hash_field(data, "acceptance_artifact");
    let package_h = parse_hash_field(data, "package_artifact");

    let store = pkg_dir.join(".genesis").join("store");
    for h in [patch_h, report_h, acceptance_h, package_h] {
        assert!(store.join(h).is_file(), "missing store artifact {h}");
    }
}
