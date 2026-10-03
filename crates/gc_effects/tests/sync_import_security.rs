#[path = "support/sync_import_controls.rs"]
mod sync_import_controls;

#[test]
fn file_backed_whole_pull_admission_and_counters_match_in_both_tiers() {
    let artifact =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/toolchain.gc");
    for compiled in [false, true] {
        let workspace = tempfile::tempdir().unwrap();
        sync_import_controls::controls(workspace.path(), &artifact, compiled);
    }
}
