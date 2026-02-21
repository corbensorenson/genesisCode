use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

#[test]
fn non_gcpm_cli_schema_registry_covers_all_stable_command_kinds() {
    let root = repo_root();
    let cli_doc = fs::read_to_string(root.join("docs/spec/CLI.md")).expect("read CLI.md");
    let schema_doc = fs::read_to_string(root.join("docs/spec/CLI_JSON_SCHEMAS_v0.1.md"))
        .expect("read CLI_JSON_SCHEMAS_v0.1.md");

    assert!(
        cli_doc.contains("docs/spec/CLI_JSON_SCHEMAS_v0.1.md"),
        "CLI.md must reference non-gcpm schema registry"
    );

    let expected_kinds = [
        "genesis/error-v0.2",
        "genesis/fmt-v0.2",
        "genesis/eval-v0.2",
        "genesis/explain-v0.2",
        "genesis/run-v0.2",
        "genesis/replay-v0.2",
        "genesis/test-v0.2",
        "genesis/pack-v0.2",
        "genesis/cli-schema-v0.1",
        "genesis/agent-index-v0.1",
        "genesis/warm-session-v0.1",
        "genesis/keygen-v0.2",
        "genesis/sign-v0.2",
        "genesis/transparency-verify-v0.2",
        "genesis/typecheck-v0.2",
        "genesis/optimize-v0.2",
        "genesis/semantic-edit-index-v0.1",
        "genesis/semantic-edit-workspace-graph-v0.1",
        "genesis/semantic-edit-refactor-plan-v0.1",
        "genesis/apply-patch-v0.2",
        "genesis/verify-v0.2",
        "genesis/selfhost-artifact-v0.2",
        "genesis/selfhost-dashboard-v0.2",
        "genesis/store-put-v0.2",
        "genesis/store-get-v0.2",
        "genesis/store-has-v0.2",
        "genesis/store-verify-v0.2",
        "genesis/refs-get-v0.1",
        "genesis/refs-list-v0.1",
        "genesis/refs-set-v0.1",
        "genesis/refs-delete-v0.1",
        "genesis/commit-new-v0.1",
        "genesis/commit-show-v0.1",
        "genesis/policy-list-v0.1",
        "genesis/policy-show-v0.1",
        "genesis/policy-set-default-v0.1",
        "genesis/sync-pull-v0.1",
        "genesis/sync-push-v0.1",
        "genesis/gc-plan-v0.1",
        "genesis/gc-run-v0.1",
        "genesis/gc-pin-v0.1",
        "genesis/gc-unpin-v0.1",
        "genesis/gc-purge-v0.1",
        "genesis/vcs-hash-v0.2",
        "genesis/vcs-diff-v0.1",
        "genesis/vcs-apply-v0.1",
        "genesis/vcs-log-v0.1",
        "genesis/vcs-blame-v0.1",
        "genesis/vcs-why-v0.1",
        "genesis/vcs-merge3-v0.1",
        "genesis/vcs-resolve-conflict-v0.1",
    ];

    for kind in expected_kinds {
        assert!(
            schema_doc.contains(kind),
            "schema registry missing kind: {kind}"
        );
    }
}

#[test]
fn gcpm_cli_schema_registry_covers_all_stable_command_kinds() {
    let root = repo_root();
    let cli_doc = fs::read_to_string(root.join("docs/spec/CLI.md")).expect("read CLI.md");
    let schema_doc = fs::read_to_string(root.join("docs/spec/GCPM_JSON_SCHEMAS_v0.1.md"))
        .expect("read GCPM_JSON_SCHEMAS_v0.1.md");

    assert!(
        cli_doc.contains("docs/spec/GCPM_JSON_SCHEMAS_v0.1.md"),
        "CLI.md must reference gcpm schema registry"
    );

    let expected_kinds = [
        "genesis/pkg-init-v0.1",
        "genesis/pkg-new-v0.1",
        "genesis/pkg-add-v0.1",
        "genesis/pkg-remove-v0.1",
        "genesis/pkg-lock-v0.1",
        "genesis/pkg-update-v0.1",
        "genesis/test-v0.2",
        "genesis/pack-v0.2",
        "genesis/typecheck-v0.2",
        "genesis/run-v0.2",
        "genesis/eval-v0.2",
        "genesis/fmt-v0.2",
        "genesis/optimize-v0.2",
        "genesis/pkg-self-optimize-v0.1",
        "genesis/pkg-runtime-profile-v0.1",
        "genesis/pkg-requirements-trace-v0.1",
        "genesis/pkg-tool-qualification-v0.1",
        "genesis/pkg-assurance-pack-v0.1",
        "genesis/pkg-install-v0.1",
        "genesis/pkg-verify-v0.1",
        "genesis/pkg-doctor-v0.1",
        "genesis/pkg-list-v0.1",
        "genesis/pkg-info-v0.1",
        "genesis/pkg-abi-v0.1",
        "genesis/pkg-snapshot-v0.1",
        "genesis/pkg-export-v0.1",
        "genesis/pkg-import-v0.1",
        "genesis/pkg-publish-v0.1",
        "genesis/pkg-migrate-v0.1",
        "genesis/pkg-env-v0.1",
    ];

    for kind in expected_kinds {
        assert!(
            schema_doc.contains(kind),
            "gcpm schema registry missing kind: {kind}"
        );
    }
}
