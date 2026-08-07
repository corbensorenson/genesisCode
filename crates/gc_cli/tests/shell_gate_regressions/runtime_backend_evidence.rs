use super::*;
use sha2::{Digest, Sha256};

#[test]
fn runtime_backend_ephemeral_target_rejects_paths_outside_report_root() {
    let root = repo_root();
    let temp = tempfile::tempdir().expect("create runtime-matrix target fixture");
    let report_root = temp.path().join("evidence");
    fs::create_dir_all(&report_root).expect("create evidence fixture root");
    let outside = temp.path().join("outside-target");
    let output = Command::new("bash")
        .arg(root.join("scripts/render_runtime_backend_feature_matrix_report.sh"))
        .arg(report_root.join("report.json"))
        .arg(report_root.join("history.jsonl"))
        .arg(report_root.join("baseline.jsonl"))
        .env(
            "GENESIS_RUNTIME_BACKEND_MATRIX_EPHEMERAL_TARGET_DIR",
            &outside,
        )
        .current_dir(&root)
        .output()
        .expect("run runtime-matrix target containment rejection");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ephemeral target must be a direct child of the report directory"),
        "runtime matrix must explain the containment violation"
    );
    assert!(
        !outside.exists(),
        "rejected runtime-matrix target must never be materialized"
    );
}

#[test]
fn runtime_backend_prebuilt_report_requires_matching_release_manifest() {
    let root = repo_root();
    let temp = tempfile::tempdir().expect("create prebuilt runtime-matrix fixture");
    let report = temp
        .path()
        .join("runtime_backend_feature_matrix_report.json");
    let manifest = temp.path().join("manifest.json");
    let report_bytes = b"{\"kind\":\"genesis/runtime-backend-feature-matrix-v0.1\",\"ok\":true}\n";
    fs::write(&report, report_bytes).expect("write prebuilt runtime-matrix report");
    let report_hash = format!("{:x}", Sha256::digest(report_bytes));
    fs::write(
        &manifest,
        format!(
            "{{\"evidence\":{{\"runtime_backend_feature_matrix_report.json\":{{\"kind\":\"genesis/runtime-backend-feature-matrix-v0.1\",\"sha256\":\"{report_hash}\"}}}},\"kind\":\"genesis/health-profile-evidence-bundle-v0.1\",\"ok\":true,\"profile\":\"release-full\"}}\n"
        ),
    )
    .expect("write prebuilt evidence manifest");

    let run = |manifest_path: Option<&Path>| {
        let mut command = Command::new("bash");
        command
            .arg(root.join("scripts/check_runtime_backend_feature_matrix.sh"))
            .env("GENESIS_CHECK_RUNTIME_BACKEND_MATRIX_REPORT", &report)
            .current_dir(&root);
        if let Some(path) = manifest_path {
            command.env("GENESIS_CHECK_RUNTIME_BACKEND_MATRIX_MANIFEST", path);
        }
        command.output().expect("run prebuilt runtime-matrix check")
    };

    assert!(run(Some(&manifest)).status.success());
    let missing_manifest = run(None);
    assert_eq!(missing_manifest.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&missing_manifest.stderr)
            .contains("prebuilt report requires GENESIS_CHECK_RUNTIME_BACKEND_MATRIX_MANIFEST")
    );

    fs::write(
        &report,
        b"{\"kind\":\"genesis/runtime-backend-feature-matrix-v0.1\",\"ok\":true,\"tampered\":true}\n",
    )
    .expect("tamper prebuilt runtime-matrix report");
    let tampered = run(Some(&manifest));
    assert_eq!(tampered.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&tampered.stderr).contains("prebuilt report hash mismatch"));
}
