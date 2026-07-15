use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("gc_cli must live under <repository>/crates")
        .to_path_buf()
}

fn copy_scenario(root: &Path, scenario: &Value, destination: &Path) {
    let scenario_root = scenario["root"].as_str().expect("scenario root");
    for file in scenario["files"].as_array().expect("scenario files") {
        let relative = file["path"].as_str().expect("fixture path");
        let source = root.join(scenario_root).join(relative);
        let target = destination.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::copy(source, target).expect("copy canonical fixture");
    }
}

fn assert_manifest_expectation(document: &Value, expectation: &Value) {
    assert_eq!(
        document["ok"], expectation["ok"],
        "stable envelope success marker drifted"
    );
    assert_eq!(
        document["kind"], expectation["kind"],
        "stable envelope kind drifted"
    );
    for assertion in expectation["assertions"].as_array().expect("assertions") {
        let pointer = assertion["pointer"].as_str().expect("JSON pointer");
        let actual = document
            .pointer(pointer)
            .unwrap_or_else(|| panic!("missing asserted JSON pointer {pointer}"));
        match assertion["operator"].as_str().expect("assertion operator") {
            "equals" => assert_eq!(
                actual, &assertion["value"],
                "exact assertion failed at {pointer}"
            ),
            "contains" => {
                let actual = actual.as_str().expect("contains target must be text");
                let expected = assertion["value"]
                    .as_str()
                    .expect("contains value must be text");
                assert!(
                    actual.contains(expected),
                    "text assertion failed at {pointer}: expected {expected:?} in {actual:?}"
                );
            }
            operator => panic!("unsupported manifest assertion operator {operator}"),
        }
    }
}

#[test]
fn canonical_language_pairs_execute_through_production_selfhost_cli() {
    let root = repository_root();
    let suite: Value = serde_json::from_slice(
        &fs::read(root.join("examples/canonical_language/v0.1/suite.json"))
            .expect("canonical suite manifest"),
    )
    .expect("canonical suite JSON");
    let artifact = root.join("selfhost/toolchain.gc");
    let pairs = suite["pairs"].as_array().expect("canonical pairs");

    assert_eq!(pairs.len(), 11, "the frozen suite must cover all concepts");
    for pair in pairs {
        let pair_id = pair["id"].as_str().expect("pair id");
        for side in ["valid", "invalid"] {
            let scenario = &pair[side];
            let workspace = tempfile::tempdir().expect("isolated canonical workspace");
            copy_scenario(&root, scenario, workspace.path());

            for step in scenario["steps"].as_array().expect("scenario steps") {
                let argv = step["argv"].as_array().expect("step argv");
                assert_eq!(argv[0], "--json", "canonical argv must use JSON mode");
                let mut command = cargo_bin_cmd!("genesis");
                command
                    .current_dir(workspace.path())
                    .arg("--json")
                    .arg("--selfhost-artifact")
                    .arg(&artifact);
                for argument in &argv[1..] {
                    command.arg(argument.as_str().expect("string argv"));
                }

                let output = command.output().unwrap_or_else(|error| {
                    panic!("failed to execute canonical case {pair_id}/{side}: {error}")
                });
                let expectation = &step["expect"];
                assert_eq!(
                    output.status.code(),
                    expectation["exitCode"].as_i64().map(|code| code as i32),
                    "exit status drifted for {pair_id}/{side}: stderr={} stdout={}",
                    String::from_utf8_lossy(&output.stderr),
                    String::from_utf8_lossy(&output.stdout)
                );
                let document: Value =
                    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
                        panic!(
                            "invalid JSON for {pair_id}/{side}: {error}; stdout={}",
                            String::from_utf8_lossy(&output.stdout)
                        )
                    });
                assert_manifest_expectation(&document, expectation);

                if !expectation["ok"].as_bool().expect("expected ok") {
                    let failure_material = serde_json::to_string(&(
                        document.get("error"),
                        document.get("diagnostics"),
                    ))
                    .expect("serialize failure material");
                    assert!(
                        !failure_material.contains(&workspace.path().display().to_string()),
                        "failure leaked its absolute workspace path for {pair_id}/{side}"
                    );
                }
            }
        }
    }
}
