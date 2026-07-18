use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

#[test]
fn source_decomposition_report_canonicalizes_host_paths() {
    let root = repo_root();
    let temp = tempfile::tempdir().expect("create source decomposition fixture");
    let policy = temp.path().join("policy.toml");
    let report = temp.path().join("report.json");
    fs::write(
        &policy,
        r#"version = 1
target_max_lines = 700
required_min_phase = "phase-1"
disallowed_statuses = ["planned", "blocked"]

[[tracked_over_budget_rows]]
module_path = "crates/gc_effects/src/runner_capability_dispatch.rs"
phase = "phase-1"
status = "waived"
parity_gate = '''python3 -c 'import pathlib, tempfile; print(pathlib.Path.home() / "private" / "input"); print(pathlib.Path(tempfile.gettempdir()) / "output.json")' '''
waiver_owner = "fixture"
waiver_scope = "portable diagnostic regression"
waiver_rationale = "fixture row exercises successful command output canonicalization"
waiver_review_by = "2099-12-31"
"#,
    )
    .expect("write source decomposition policy fixture");

    let output = Command::new("bash")
        .arg(root.join("scripts/render_source_decomposition_tracked_parity_report.sh"))
        .arg(&report)
        .arg(&policy)
        .env("GENESIS_SOURCE_DECOMPOSITION_REVIEW_DATE", "2026-07-10")
        .current_dir(&root)
        .output()
        .expect("render source decomposition fixture");
    assert!(
        output.status.success(),
        "source decomposition fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report_text = fs::read_to_string(&report).expect("read source decomposition report");
    assert!(
        !report_text.contains("/Users/"),
        "report leaked a user path"
    );
    assert!(
        !report_text.contains("/var/folders/"),
        "report leaked a temporary path"
    );
    assert!(
        report_text.contains("<host-path>"),
        "report did not retain a portable path marker"
    );
}
