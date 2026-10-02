#![cfg(unix)]
//! Local source-bound public controls, not independent host qualification.
#[path = "support/document_reader_controls.rs"]
mod document_reader_controls;

#[test]
fn native_patch_and_profile_document_reads_preserve_authority_and_limits() {
    for compiled in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("patch.gc"), "{:type :vcs/patch :v 1 :ops []}").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        let artifact =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/toolchain.gc");
        document_reader_controls::controls(&root, &artifact, compiled);
        assert_eq!(
            std::fs::read_to_string(outside.join("patch.gc")).unwrap(),
            "{:type :vcs/patch :v 1 :ops []}"
        );
    }
}
