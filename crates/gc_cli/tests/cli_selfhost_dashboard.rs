use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

#[test]
fn selfhost_dashboard_writes_store_artifact_and_markdown_mirror() {
    let td = tempdir().unwrap();
    let store = td.path().join("store");
    let markdown = td.path().join("status").join("SELFHOST_CUTOVER.md");

    let out = cargo_bin_cmd!("genesis")
        .args([
            "--json",
            "selfhost-dashboard",
            "--store",
            store.to_str().unwrap(),
            "--markdown",
            markdown.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let data = v.get("data").unwrap();

    let artifact_hash = data
        .get("artifact_hash")
        .and_then(JsonValue::as_str)
        .unwrap();
    assert_eq!(artifact_hash.len(), 64);
    assert!(store.join(artifact_hash).is_file());
    assert!(markdown.is_file());

    let summary = data.get("summary").unwrap();
    let total = summary
        .get("total_commands")
        .and_then(JsonValue::as_u64)
        .unwrap();
    let routed = summary
        .get("selfhost_routed_commands")
        .and_then(JsonValue::as_u64)
        .unwrap();
    assert!(total > 0);
    assert!(routed <= total);

    let fast_path_ok = summary
        .get("fast_path_default_ok")
        .and_then(JsonValue::as_bool)
        .unwrap();
    assert!(fast_path_ok, "fast-path default routing must be selfhost");

    let md = std::fs::read_to_string(markdown).unwrap();
    assert!(md.contains("Selfhost Cutover Dashboard"));
    assert!(md.contains("`policy/*`"));
    assert!(md.contains("`store/*` | true | true | true"));
}
