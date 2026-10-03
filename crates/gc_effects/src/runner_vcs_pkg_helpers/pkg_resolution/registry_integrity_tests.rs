use super::*;

#[test]
fn package_hydration_preserves_store_mismatch_and_inventory() {
    let td = tempfile::tempdir().unwrap();
    let remote_dir = td.path().join("remote");
    let remote = format!("file://{}/", remote_dir.display());
    let expected = hash_bytes_hex(b"honest");
    std::fs::create_dir_all(remote_dir.join("v1/store")).unwrap();
    std::fs::write(remote_dir.join("v1/store").join(&expected), b"corrupt").unwrap();
    let store = ArtifactStore::open(&td.path().join("local")).unwrap();
    let artifact =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/toolchain.gc");
    let policy = CapsPolicy::from_toml_str_with_selfhost_authority(
        &format!(
            r#"
allow = ["core/pkg-low::install"]
[store]
remote = "{remote}"
remote_allow = ["{remote}"]
"#
        ),
        gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();
    let mut context = gc_kernel::EvalCtx::new();
    gc_prelude::build_prelude(&mut context);
    let error_token = context.protocol.expect("prelude protocol").error;
    let mut budget = ArtifactBudgetState::default();
    let error = ensure_artifact_hash_available(
        &store,
        &BTreeMap::new(),
        None,
        &policy,
        policy.op_policy("core/pkg-low::install"),
        &mut budget,
        Some(1000),
        &expected,
        error_token,
        "core/pkg-low::install",
    )
    .unwrap_err();
    let Value::Sealed { token, payload } = error else {
        panic!("expected sealed error")
    };
    assert_eq!(token, error_token);
    let Term::Map(fields) = payload.to_plain_term().unwrap() else {
        panic!("expected error fields")
    };
    assert_eq!(
        fields.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("core/store/hash-mismatch".to_string()))
    );
    assert_eq!(
        fields.get(&TermOrdKey(Term::symbol(":error/message"))),
        Some(&Term::Str("remote bytes hash mismatch".to_string()))
    );
    assert_eq!(
        std::fs::read_dir(td.path().join("local")).unwrap().count(),
        0
    );
    assert_eq!(budget.store_written_bytes, 0);
}
