use gc_vcs::{GpkError, GpkReadLimits, read_bundle, read_bundle_with_limits, write_bundle};

fn hash_bytes32(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}

#[test]
fn gpk_v1_rejects_trailing_bytes() {
    let root = hash_bytes32(b"root");
    let ent_bytes = b"{:kind \"x\"}".to_vec();
    let ent_hex = blake3::hash(&ent_bytes).to_hex().to_string();

    let mut buf: Vec<u8> = Vec::new();
    write_bundle(&mut buf, 1, root, &[(ent_hex, ent_bytes)], None).unwrap();
    buf.push(0xAA);

    let err = read_bundle(std::io::Cursor::new(buf)).unwrap_err();
    match err {
        GpkError::BadIndex(s) => assert!(s.contains("trailing")),
        other => panic!("expected trailing bytes error, got {other:?}"),
    }
}

#[test]
fn gpk_v2_roundtrips_refs_and_sorts_them() {
    let root = hash_bytes32(b"root");
    let ent_bytes = b"{:kind \"x\"}".to_vec();
    let ent_hex = blake3::hash(&ent_bytes).to_hex().to_string();
    let refs = vec![
        (
            "refs/tags/v1.0.0".to_string(),
            blake3::hash(b"c2").to_hex().to_string(),
        ),
        (
            "refs/heads/main".to_string(),
            blake3::hash(b"c1").to_hex().to_string(),
        ),
    ];

    let mut buf: Vec<u8> = Vec::new();
    write_bundle(&mut buf, 2, root, &[(ent_hex, ent_bytes)], Some(&refs)).unwrap();
    let b = read_bundle(std::io::Cursor::new(buf)).unwrap();
    assert_eq!(b.version, 2);
    assert_eq!(b.refs.len(), 2);
    assert_eq!(b.refs[0].name, "refs/heads/main");
    assert_eq!(b.refs[1].name, "refs/tags/v1.0.0");
}

#[test]
fn gpk_v1_write_rejects_refs_section() {
    let root = hash_bytes32(b"root");
    let ent_bytes = b"{:kind \"x\"}".to_vec();
    let ent_hex = blake3::hash(&ent_bytes).to_hex().to_string();
    let refs = vec![(
        "refs/heads/main".to_string(),
        blake3::hash(b"c1").to_hex().to_string(),
    )];

    let mut buf: Vec<u8> = Vec::new();
    let err = write_bundle(&mut buf, 1, root, &[(ent_hex, ent_bytes)], Some(&refs)).unwrap_err();
    match err {
        GpkError::BadIndex(_) => {}
        other => panic!("expected BadIndex, got {other:?}"),
    }
}

#[test]
fn gpk_read_limits_reject_entry_count_overflow() {
    let root = hash_bytes32(b"root");
    let ent_a = b"{:kind \"a\"}".to_vec();
    let ent_b = b"{:kind \"b\"}".to_vec();
    let hex_a = blake3::hash(&ent_a).to_hex().to_string();
    let hex_b = blake3::hash(&ent_b).to_hex().to_string();

    let mut buf: Vec<u8> = Vec::new();
    write_bundle(&mut buf, 1, root, &[(hex_a, ent_a), (hex_b, ent_b)], None).unwrap();

    let lim = GpkReadLimits {
        max_entries: 1,
        ..GpkReadLimits::default_hard()
    };
    let err = read_bundle_with_limits(std::io::Cursor::new(buf), &lim).unwrap_err();
    match err {
        GpkError::LimitExceeded(s) => assert!(s.contains("entry count")),
        other => panic!("expected LimitExceeded, got {other:?}"),
    }
}

#[test]
fn gpk_read_limits_reject_entry_size_overflow() {
    let root = hash_bytes32(b"root");
    let ent = vec![0xAB; 4096];
    let hex = blake3::hash(&ent).to_hex().to_string();

    let mut buf: Vec<u8> = Vec::new();
    write_bundle(&mut buf, 1, root, &[(hex, ent)], None).unwrap();

    let lim = GpkReadLimits {
        max_entry_bytes: 512,
        ..GpkReadLimits::default_hard()
    };
    let err = read_bundle_with_limits(std::io::Cursor::new(buf), &lim).unwrap_err();
    match err {
        GpkError::LimitExceeded(s) => assert!(s.contains("per-entry limit")),
        other => panic!("expected LimitExceeded, got {other:?}"),
    }
}
