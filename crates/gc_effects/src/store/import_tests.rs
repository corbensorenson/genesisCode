use super::*;

fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn inventory(path: &std::path::Path) -> std::collections::BTreeMap<std::ffi::OsString, Vec<u8>> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name(), std::fs::read(entry.path()).unwrap())
        })
        .collect()
}

#[test]
fn rejected_and_dropped_imports_preserve_existing_inventory() {
    let td = tempfile::tempdir().unwrap();
    let store = ArtifactStore::open(td.path()).unwrap();
    let retained = store.put_bytes(b"retained").unwrap();
    let before = inventory(td.path());
    let mut import = ArtifactImport::new(&store, 32, Some(64), 4);
    import.stage(&hash(b"honest"), b"honest").unwrap();
    assert_eq!(import.get_bytes(&hash(b"honest")).unwrap(), b"honest");
    assert_eq!(import.get_bytes(&retained).unwrap(), b"retained");
    assert!(matches!(
        import.stage(&hash(b"expected"), b"wrong"),
        Err(ImportError::Identity(
            gc_registry::RegistryError::HashMismatch { .. }
        ))
    ));
    drop(import);
    assert_eq!(inventory(td.path()), before);
}

#[test]
fn import_limits_precede_destination_mutation_and_duplicate_accounting() {
    let td = tempfile::tempdir().unwrap();
    let store = ArtifactStore::open(td.path()).unwrap();
    let mut import = ArtifactImport::new(&store, 4, Some(4), 1);
    import.stage(&hash(b"four"), b"four").unwrap();
    import.stage(&hash(b"four"), b"four").unwrap();
    assert_eq!(import.staged_bytes(), 4);
    assert!(matches!(
        import.stage(&hash(b"x"), b"x"),
        Err(ImportError::ResourceLimit(_))
    ));
    assert!(matches!(
        import.stage(&hash(b"five!"), b"five!"),
        Err(ImportError::ResourceLimit(_))
    ));
    drop(import);
    assert!(inventory(td.path()).is_empty());
    let mut objects = ArtifactImport::new(&store, 4, Some(32), 1);
    objects.stage(&hash(b"a"), b"a").unwrap();
    assert!(matches!(
        objects.stage(&hash(b"b"), b"b"),
        Err(ImportError::ResourceLimit(_))
    ));
    drop(objects);
    assert!(inventory(td.path()).is_empty());
}

#[test]
fn spool_corruption_is_preflighted_before_any_installation() {
    let td = tempfile::tempdir().unwrap();
    let store = ArtifactStore::open(td.path()).unwrap();
    let mut import = ArtifactImport::new(&store, 32, Some(64), 4);
    import.stage(&hash(b"first"), b"first").unwrap();
    import.stage(&hash(b"second"), b"second").unwrap();
    let range = import.index[blake3::hash(b"second").as_bytes()];
    let file = import.spool.as_mut().unwrap();
    file.seek(SeekFrom::Start(range.offset)).unwrap();
    file.write_all(b"broken").unwrap();
    let (mut written, mut pulled) = (7, 0);
    assert!(matches!(
        import.publish(&mut written, &mut pulled),
        Err(ImportError::Identity(
            gc_registry::RegistryError::HashMismatch { .. }
        ))
    ));
    assert_eq!((written, pulled), (7, 0));
    assert!(inventory(td.path()).is_empty());
}

#[test]
fn admitted_import_preserves_concurrent_identical_objects_and_charges_completed_writes() {
    let td = tempfile::tempdir().unwrap();
    let store = ArtifactStore::open(td.path()).unwrap();
    let mut import = ArtifactImport::new(&store, 32, Some(64), 4);
    import.stage(&hash(b"first"), b"first").unwrap();
    import.stage(&hash(b"second"), b"second").unwrap();
    let existing = store.put_bytes(b"second").unwrap();
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(store.path_for(&existing)).unwrap().ino()
    };
    let (mut written, mut pulled) = (7, 0);
    import.publish(&mut written, &mut pulled).unwrap();
    assert_eq!((written, pulled), (18, 2));
    assert_eq!(store.get_bytes(&hash(b"first")).unwrap(), b"first");
    assert_eq!(store.get_bytes(&existing).unwrap(), b"second");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::metadata(store.path_for(&existing)).unwrap().ino(),
            inode
        );
    }
    assert_eq!(inventory(td.path()).len(), 2);
}

#[test]
fn malformed_names_never_reach_destination_path_lookup() {
    let td = tempfile::tempdir().unwrap();
    let store = ArtifactStore::open(td.path()).unwrap();
    let mut import = ArtifactImport::new(&store, 32, Some(64), 4);
    for name in ["../outside", "", &"A".repeat(64), &"0".repeat(63)] {
        assert!(matches!(
            import.contains(name),
            Err(ImportError::Identity(gc_registry::RegistryError::Protocol(
                _
            )))
        ));
        assert!(matches!(
            import.get_bytes(name),
            Err(ImportError::Identity(gc_registry::RegistryError::Protocol(
                _
            )))
        ));
    }
    assert!(inventory(td.path()).is_empty());
}

#[test]
fn publication_io_failure_charges_earlier_completed_writes_without_deleting_them() {
    let td = tempfile::tempdir().unwrap();
    let store = ArtifactStore::open(td.path()).unwrap();
    let mut import = ArtifactImport::new(&store, 32, Some(64), 4);
    import.stage(&hash(b"first"), b"first").unwrap();
    import.stage(&hash(b"second"), b"second").unwrap();
    std::fs::create_dir(store.path_for(&hash(b"second"))).unwrap();
    let (mut written, mut pulled) = (7, 0);
    assert!(import.publish(&mut written, &mut pulled).is_err());
    assert_eq!((written, pulled), (12, 1));
    assert_eq!(store.get_bytes(&hash(b"first")).unwrap(), b"first");
    assert!(store.path_for(&hash(b"second")).is_dir());
    assert!(std::fs::read_dir(td.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".genesis-import")
    }));
}

#[test]
fn empty_objects_and_exact_byte_budget_publish_without_duplicate_growth() {
    let td = tempfile::tempdir().unwrap();
    let store = ArtifactStore::open(td.path()).unwrap();
    let mut import = ArtifactImport::new(&store, 4, Some(4), 2);
    import.stage(&hash(b""), b"").unwrap();
    import.stage(&hash(b"four"), b"four").unwrap();
    import.stage(&hash(b""), b"").unwrap();
    let (mut written, mut pulled) = (0, 0);
    import.publish(&mut written, &mut pulled).unwrap();
    assert_eq!((written, pulled), (4, 2));
    assert_eq!(store.get_bytes(&hash(b"")).unwrap(), b"");
    assert_eq!(store.get_bytes(&hash(b"four")).unwrap(), b"four");
}

#[cfg(unix)]
#[test]
fn native_and_wasi_descriptor_spools_preserve_occupied_links_and_held_roots() {
    use std::os::unix::fs::symlink;
    type Acquire = fn(&std::path::Path) -> std::io::Result<File>;
    let acquire: [Acquire; 2] = [
        |path| crate::rooted_fs::FsRoot::open(path)?.scratch_file(),
        |path| crate::wasi_rooted_descriptor_controls::FsRoot::open(path)?.scratch_file(),
    ];
    for acquire in acquire {
        let td = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("retained"), b"retained").unwrap();
        let collision = td.path().join(format!(
            ".genesis-import.{}.0.tmp",
            crate::platform_process_id()
        ));
        symlink(outside.path().join("retained"), &collision).unwrap();
        let mut file = acquire(td.path()).unwrap();
        file.write_all(b"private").unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut body = String::new();
        file.read_to_string(&mut body).unwrap();
        assert_eq!(body, "private");
        drop(file);
        assert_eq!(std::fs::read_dir(td.path()).unwrap().count(), 1);
        assert_eq!(
            std::fs::read_link(collision).unwrap(),
            outside.path().join("retained")
        );
        assert_eq!(
            std::fs::read(outside.path().join("retained")).unwrap(),
            b"retained"
        );
    }
    let td = tempfile::tempdir().unwrap();
    let inside = td.path().join("inside");
    let held = td.path().join("held");
    let outside = td.path().join("outside");
    std::fs::create_dir(&inside).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let native = crate::rooted_fs::FsRoot::open(&inside).unwrap();
    let wasi = crate::wasi_rooted_descriptor_controls::FsRoot::open(&inside).unwrap();
    std::fs::rename(&inside, &held).unwrap();
    symlink(&outside, &inside).unwrap();
    for mut file in [native.scratch_file().unwrap(), wasi.scratch_file().unwrap()] {
        file.write_all(b"held").unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut body = String::new();
        file.read_to_string(&mut body).unwrap();
        assert_eq!(body, "held");
    }
    assert_eq!(std::fs::read_dir(&held).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
}
