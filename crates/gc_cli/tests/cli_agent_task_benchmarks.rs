use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("gc_cli must live under <repository>/crates")
        .to_path_buf()
}

fn copy_files(root: &Path, tree_root: &str, files: &Value, destination: &Path) {
    for file in files.as_array().expect("benchmark file list") {
        let relative = file["path"].as_str().expect("benchmark file path");
        let source = root.join(tree_root).join(relative);
        let target = destination.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("create benchmark parent");
        }
        fs::copy(source, target).expect("copy benchmark file");
    }
}

fn run_json(root: &Path, workspace: &Path, argv: &Value) -> (i32, Value) {
    let argv = argv.as_array().expect("benchmark argv");
    assert_eq!(argv[0], "--json", "benchmark commands must use JSON mode");
    let mut command = cargo_bin_cmd!("genesis");
    command
        .current_dir(workspace)
        .arg("--json")
        .arg("--selfhost-artifact")
        .arg(root.join("selfhost/toolchain.gc"));
    for argument in &argv[1..] {
        command.arg(argument.as_str().expect("string benchmark argument"));
    }
    let output = command.output().expect("execute benchmark command");
    let document = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "benchmark command returned invalid JSON: {error}; stderr={}; stdout={}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (output.status.code().unwrap_or(-1), document)
}

fn apply_source_append(workspace: &Path, editable_paths: &Value, source_append: &Value) {
    if source_append.is_null() {
        return;
    }
    let relative_raw = source_append["path"].as_str().expect("source append path");
    assert!(
        editable_paths
            .as_array()
            .expect("editable paths")
            .iter()
            .any(|path| path.as_str() == Some(relative_raw)),
        "source append path must be in the case editable surface: {relative_raw}"
    );
    let relative = Path::new(relative_raw);
    assert!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "source append path must be a non-empty safe relative path: {relative_raw}"
    );

    let mut target = workspace.to_path_buf();
    for component in relative.components() {
        target.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&target).unwrap_or_else(|error| {
            panic!(
                "source append target component is unavailable: {}; {error}",
                target.display()
            )
        });
        assert!(
            !metadata.file_type().is_symlink(),
            "source append target cannot traverse a symlink: {}",
            target.display()
        );
    }
    assert!(target.is_file(), "source append target must be a file");
    OpenOptions::new()
        .append(true)
        .open(&target)
        .expect("open source append target")
        .write_all(
            source_append["source"]
                .as_str()
                .expect("source append contents")
                .as_bytes(),
        )
        .expect("append benchmark source");
}

fn assert_step(root: &Path, workspace: &Path, task: &str, editable_paths: &Value, step: &Value) {
    apply_source_append(workspace, editable_paths, &step["sourceAppend"]);
    let (code, document) = run_json(root, workspace, &step["argv"]);
    assert_eq!(
        code, step["exitCode"],
        "benchmark exit code drifted for {task}/{}: {document}",
        step["id"]
    );
    assert_eq!(
        document["ok"], step["ok"],
        "benchmark ok marker drifted for {task}/{}",
        step["id"]
    );
    assert_eq!(
        document["kind"], step["kind"],
        "benchmark kind drifted for {task}/{}",
        step["id"]
    );
    for assertion in step["assertions"].as_array().expect("assertions") {
        let pointer = assertion["pointer"].as_str().expect("JSON pointer");
        let actual = document
            .pointer(pointer)
            .unwrap_or_else(|| panic!("missing benchmark pointer {pointer}"));
        match assertion["operator"].as_str().expect("assertion operator") {
            "equals" => assert_eq!(
                actual, &assertion["value"],
                "assertion failed for {task}/{} at {pointer}",
                step["id"]
            ),
            "contains" => assert!(
                actual
                    .as_str()
                    .expect("contains target")
                    .contains(assertion["value"].as_str().expect("contains value")),
                "contains assertion failed for {task}/{} at {pointer}",
                step["id"]
            ),
            operator => panic!("unsupported benchmark assertion {operator}"),
        }
    }
}

#[test]
fn task_benchmark_matrix_executes_public_references_through_production_cli() {
    let root = repository_root();
    let suite: Value = serde_json::from_slice(
        &fs::read(root.join("benchmarks/agent_tasks/v0.1/suite.json"))
            .expect("task benchmark manifest"),
    )
    .expect("task benchmark JSON");
    let cases = suite["cases"].as_array().expect("benchmark cases");
    assert_eq!(cases.len(), 27);
    let lineages = suite["lineages"].as_array().expect("benchmark lineages");
    let conditions = suite["conditions"]
        .as_array()
        .expect("benchmark conditions");
    assert_eq!(lineages.len(), 9);
    assert_eq!(conditions.len(), 27);
    let lineage_ids: BTreeSet<_> = lineages
        .iter()
        .map(|row| row["id"].as_str().expect("lineage id"))
        .collect();
    assert_eq!(lineage_ids.len(), 9);
    for lineage in lineages {
        let children: Vec<_> = conditions
            .iter()
            .filter(|row| row["lineageId"] == lineage["id"])
            .collect();
        assert_eq!(children.len(), 3);
        assert!(
            children
                .iter()
                .all(|row| row["lineageIdentitySha256"] == lineage["contentIdentitySha256"])
        );
    }
    for case in cases {
        let lineage = lineages
            .iter()
            .find(|row| row["id"] == case["lineageId"])
            .expect("case lineage");
        let condition = conditions
            .iter()
            .find(|row| row["id"] == case["conditionId"])
            .expect("case condition");
        assert_eq!(
            case["lineageIdentitySha256"],
            lineage["contentIdentitySha256"]
        );
        assert_eq!(
            case["conditionIdentitySha256"],
            condition["contentIdentitySha256"]
        );
    }

    let mut matrix: BTreeMap<String, Vec<(String, u64)>> = BTreeMap::new();
    for case in cases {
        matrix
            .entry(case["taskClass"].as_str().expect("task class").to_owned())
            .or_default()
            .push((
                case["contextTier"]
                    .as_str()
                    .expect("context tier")
                    .to_owned(),
                case["contextBytes"].as_u64().expect("context bytes"),
            ));
    }
    assert_eq!(matrix.len(), 9);
    for tiers in matrix.values() {
        assert_eq!(
            tiers.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
            ["small", "medium", "large"]
        );
        assert!(tiers.windows(2).all(|pair| pair[0].1 < pair[1].1));
    }

    let mut executed = BTreeSet::new();
    for case in cases {
        let task = case["taskClass"].as_str().expect("task class");
        if case["contextTier"] != "small" || !executed.insert(task.to_owned()) {
            continue;
        }
        let workspace = tempfile::tempdir().expect("isolated benchmark workspace");
        copy_files(
            &root,
            case["inputRoot"].as_str().expect("input root"),
            &case["inputFiles"],
            workspace.path(),
        );

        if task == "refactor" {
            let (_, baseline) = run_json(&root, workspace.path(), &case["verification"][0]["argv"]);
            assert_eq!(baseline["data"]["value"], "42");
        }
        if task == "performance-repair" {
            let (code, baseline) =
                run_json(&root, workspace.path(), &case["verification"][0]["argv"]);
            assert_ne!(code, 0, "performance input must exceed the finite budget");
            assert_eq!(baseline["error"]["code"], "eval/error");
        }
        if task == "package-migration" {
            let (code, baseline) =
                run_json(&root, workspace.path(), &case["verification"][0]["argv"]);
            assert_eq!(
                code, 10,
                "schema-2 package must be rejected before migration"
            );
            assert_eq!(baseline["error"]["code"], "manifest/error");
        }

        copy_files(
            &root,
            case["referenceRoot"].as_str().expect("reference root"),
            &case["referenceFiles"],
            workspace.path(),
        );
        for step in case["verification"].as_array().expect("verification steps") {
            assert_step(&root, workspace.path(), task, &case["editablePaths"], step);
        }

        if task == "policy-minimization" {
            assert_eq!(
                fs::read_to_string(workspace.path().join("caps.toml")).expect("policy"),
                "allow = [\"io/fs::read\"]\n\n[op.\"io/fs::read\"]\nbase_dir = \".\"\n"
            );
        }
        if task == "replay-investigation" {
            let finding: Value = serde_json::from_slice(
                &fs::read(workspace.path().join("finding.json")).expect("replay finding"),
            )
            .expect("replay finding JSON");
            assert_eq!(finding["entryIndex"], 0);
            assert_eq!(finding["code"], "replay/mismatch");
            assert_eq!(finding["classification"], "response-hash-divergence");
        }
        if task == "deployment" {
            let plan: Value = serde_json::from_slice(
                &fs::read(workspace.path().join("deployment.json")).expect("deployment plan"),
            )
            .expect("deployment plan JSON");
            assert_eq!(plan["target"], "service");
            assert!(workspace.path().join("dist/service").is_dir());
        }
    }
    assert_eq!(executed.len(), 9);
}
