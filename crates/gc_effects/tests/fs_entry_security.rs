#![cfg(unix)]
//! Source-bound host safety controls; not a host qualification or native oracle.
use gc_coreform::{hash_module, parse_module};
use gc_effects::{CapsPolicy, run};
use gc_kernel::{EvalCtx, Value, eval_module, eval_module_compiled};
use gc_prelude::build_prelude;
use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

type EntrySnapshot = (u64, u64, Vec<u8>);

fn snapshot(root: &Path) -> BTreeMap<std::path::PathBuf, EntrySnapshot> {
    fn walk(root: &Path, path: &Path, out: &mut BTreeMap<std::path::PathBuf, EntrySnapshot>) {
        let metadata = std::fs::symlink_metadata(path).unwrap();
        let identity = path.strip_prefix(root).unwrap().to_path_buf();
        if metadata.file_type().is_symlink() {
            out.insert(
                identity,
                (
                    metadata.dev(),
                    metadata.ino(),
                    format!("link:{:?}", std::fs::read_link(path).unwrap()).into_bytes(),
                ),
            );
        } else if metadata.is_dir() {
            out.insert(
                identity,
                (metadata.dev(), metadata.ino(), b"directory".to_vec()),
            );
            for entry in std::fs::read_dir(path).unwrap() {
                walk(root, &entry.unwrap().path(), out);
            }
        } else {
            out.insert(
                identity,
                (metadata.dev(), metadata.ino(), std::fs::read(path).unwrap()),
            );
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn perform(
    base: &Path,
    operation: &str,
    payload: &str,
    compiled: bool,
) -> Result<Value, gc_effects::EffectsError> {
    let policy = CapsPolicy::from_toml_str(&format!(
        "allow = [\"{operation}\"]\n[op.\"{operation}\"]\nbase_dir = {:?}\ncreate_dirs = true\n",
        base.to_str().unwrap()
    ))
    .unwrap();
    let forms = parse_module(&format!(
        "(core/effect::perform '{operation} {payload} (fn (response) (core/effect::pure response)))"
    ))
    .unwrap();
    let mut context = EvalCtx::with_step_limit(Some(2_000_000));
    let mut env = build_prelude(&mut context).env;
    let program = if compiled {
        eval_module_compiled(&mut context, &mut env, &forms)
    } else {
        eval_module(&mut context, &mut env, &forms)
    }
    .unwrap();
    run(
        &mut context,
        &policy,
        program,
        hash_module(&forms),
        "fs-entry-security".to_string(),
    )
    .map(|result| result.value)
}

#[test]
fn rejected_escaping_ancestor_never_creates_directories_for_any_mutator() {
    let mut violations = Vec::new();
    for compiled in [false, true] {
        for (operation, payload) in [
            (
                "io/fs::write",
                "{:path \"escape/new/entry\" :data \"forbidden\"}",
            ),
            ("io/fs::mkdir", "{:path \"escape/new/entry\" :parents true}"),
            (
                "io/fs::rename",
                "{:from \"source\" :to \"escape/new/entry\" :overwrite true}",
            ),
        ] {
            let fixture = tempfile::tempdir().unwrap();
            let sandbox = fixture.path().join("sandbox");
            let outside = fixture.path().join("outside");
            std::fs::create_dir(&sandbox).unwrap();
            std::fs::create_dir(&outside).unwrap();
            std::fs::write(sandbox.join("source"), b"retained source").unwrap();
            std::os::unix::fs::symlink(&outside, sandbox.join("escape")).unwrap();
            let before = (snapshot(&sandbox), snapshot(&outside));
            let result = perform(&sandbox, operation, payload, compiled);
            let denied = result.is_err() || matches!(result, Ok(Value::Sealed { .. }));
            let unchanged = (snapshot(&sandbox), snapshot(&outside)) == before;
            if !denied || !unchanged {
                violations.push(format!(
                    "{operation}, compiled={compiled}, denied={denied}, unchanged={unchanged}"
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "escaping ancestor violations: {violations:?}"
    );
}

#[test]
fn rename_same_entry_with_overwrite_preserves_source() {
    let mut violations = Vec::new();
    for compiled in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::write(fixture.path().join("source"), b"retained source").unwrap();
        let before = snapshot(fixture.path());
        let result = perform(
            fixture.path(),
            "io/fs::rename",
            "{:from \"source\" :to \"source\" :overwrite true}",
            compiled,
        );
        let succeeded = matches!(
            result.as_ref().ok().and_then(Value::as_data),
            Some(gc_coreform::Term::Nil)
        );
        let unchanged = snapshot(fixture.path()) == before;
        if !succeeded || !unchanged {
            violations.push(format!(
                "compiled={compiled}, succeeded={succeeded}, unchanged={unchanged}"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "same-entry rename violations: {violations:?}"
    );
}

#[test]
fn remove_final_link_preserves_its_file_target() {
    let mut violations = Vec::new();
    for compiled in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::write(fixture.path().join("target"), b"retained target").unwrap();
        std::os::unix::fs::symlink("target", fixture.path().join("link")).unwrap();
        let result = perform(
            fixture.path(),
            "io/fs::remove",
            "{:path \"link\" :recursive true}",
            compiled,
        );
        let succeeded = matches!(
            result.as_ref().ok().and_then(Value::as_data),
            Some(gc_coreform::Term::Nil)
        );
        let target_retained = std::fs::read(fixture.path().join("target")).ok().as_deref()
            == Some(b"retained target".as_slice());
        let link_removed = std::fs::symlink_metadata(fixture.path().join("link")).is_err();
        if !succeeded || !target_retained || !link_removed {
            violations.push(format!("compiled={compiled}, succeeded={succeeded}, target_retained={target_retained}, link_removed={link_removed}"));
        }
    }
    assert!(
        violations.is_empty(),
        "final-link removal violations: {violations:?}"
    );
}

fn is_nil(result: &Result<Value, gc_effects::EffectsError>) -> bool {
    matches!(
        result.as_ref().ok().and_then(Value::as_data),
        Some(gc_coreform::Term::Nil)
    )
}

#[test]
fn remove_stat_and_missing_entries_preserve_every_link_target() {
    for compiled in [false, true] {
        for target_kind in ["file", "directory", "dangling", "outside"] {
            for recursive in [false, true] {
                let fixture = tempfile::tempdir().unwrap();
                let sandbox = fixture.path().join("sandbox");
                let outside = fixture.path().join("outside");
                std::fs::create_dir(&sandbox).unwrap();
                std::fs::create_dir(&outside).unwrap();
                std::fs::write(sandbox.join("file"), b"inside file").unwrap();
                std::fs::create_dir(sandbox.join("directory")).unwrap();
                std::fs::write(sandbox.join("directory/retained"), b"inside directory").unwrap();
                std::fs::write(outside.join("retained"), b"outside file").unwrap();
                let target = if target_kind == "outside" {
                    outside.clone()
                } else {
                    std::path::PathBuf::from(target_kind)
                };
                std::os::unix::fs::symlink(target, sandbox.join("link")).unwrap();
                let mut expected = snapshot(&sandbox);
                expected.remove(Path::new("link"));
                let outside_before = snapshot(&outside);
                let stat = perform(&sandbox, "io/fs::stat", "{:path \"link\"}", compiled).unwrap();
                let gc_coreform::Term::Map(fields) = stat.as_data().unwrap() else {
                    panic!("stat envelope");
                };
                assert_eq!(
                    fields.get(&gc_coreform::TermOrdKey(gc_coreform::Term::symbol(":kind"))),
                    Some(&gc_coreform::Term::symbol("symlink"))
                );
                let result = perform(
                    &sandbox,
                    "io/fs::remove",
                    &format!("{{:path \"link\" :recursive {recursive}}}"),
                    compiled,
                );
                assert!(
                    is_nil(&result),
                    "{target_kind}, recursive={recursive}, compiled={compiled}: {result:?}"
                );
                assert_eq!(snapshot(&sandbox), expected);
                assert_eq!(snapshot(&outside), outside_before);
            }
        }
        let fixture = tempfile::tempdir().unwrap();
        let before = snapshot(fixture.path());
        let result = perform(
            fixture.path(),
            "io/fs::remove",
            "{:path \"missing/parent/entry\" :recursive true}",
            compiled,
        );
        assert!(
            is_nil(&result),
            "missing parent must be a no-op: {result:?}"
        );
        assert_eq!(snapshot(fixture.path()), before);
    }
}

#[test]
fn inside_relative_and_absolute_links_support_regular_operations() {
    for compiled in [false, true] {
        for absolute in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let root = fixture.path();
            std::fs::create_dir(root.join("inside")).unwrap();
            std::fs::write(root.join("inside/input"), b"read value").unwrap();
            std::fs::write(root.join("source"), b"rename value").unwrap();
            let target = if absolute {
                root.join("inside")
            } else {
                std::path::PathBuf::from("inside")
            };
            std::os::unix::fs::symlink(target, root.join("link")).unwrap();
            let read = perform(root, "io/fs::read", "{:path \"link/input\"}", compiled).unwrap();
            assert!(
                matches!(read.as_data(), Some(gc_coreform::Term::Bytes(bytes)) if bytes.as_ref() == b"read value"),
                "absolute={absolute}, compiled={compiled}, read={read:?}"
            );
            for (op, payload) in [
                ("io/fs::mkdir", "{:path \"link/new\" :parents true}"),
                (
                    "io/fs::write",
                    "{:path \"link/new/output\" :data \"write value\"}",
                ),
                ("io/fs::rename", "{:from \"source\" :to \"link/renamed\"}"),
            ] {
                let result = perform(root, op, payload, compiled);
                assert!(
                    is_nil(&result),
                    "{op}, absolute={absolute}, compiled={compiled}: {result:?}"
                );
            }
            assert_eq!(
                std::fs::read(root.join("inside/new/output")).unwrap(),
                b"write value"
            );
            assert_eq!(
                std::fs::read(root.join("inside/renamed")).unwrap(),
                b"rename value"
            );
            assert!(!root.join("source").exists());
            assert!(
                std::fs::symlink_metadata(root.join("link"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
    }
}

#[test]
fn rejected_rename_never_removes_existing_source_or_destination() {
    for compiled in [false, true] {
        for kind in [
            "missing-source",
            "file-directory",
            "directory-file",
            "occupied-directory",
            "no-overwrite",
            "dangling-destination",
        ] {
            let fixture = tempfile::tempdir().unwrap();
            let root = fixture.path();
            if matches!(kind, "directory-file" | "occupied-directory") {
                std::fs::create_dir(root.join("source")).unwrap();
                std::fs::write(root.join("source/retained"), b"source directory").unwrap();
            } else if kind != "missing-source" {
                std::fs::write(root.join("source"), b"source file").unwrap();
            }
            match kind {
                "file-directory" | "occupied-directory" => {
                    std::fs::create_dir(root.join("destination")).unwrap();
                    std::fs::write(root.join("destination/retained"), b"destination directory")
                        .unwrap();
                }
                "dangling-destination" => {
                    std::os::unix::fs::symlink("absent", root.join("destination")).unwrap()
                }
                _ => std::fs::write(root.join("destination"), b"destination file").unwrap(),
            }
            let before = snapshot(root);
            let overwrite = !matches!(kind, "no-overwrite" | "dangling-destination");
            let result = perform(
                root,
                "io/fs::rename",
                &format!("{{:from \"source\" :to \"destination\" :overwrite {overwrite}}}"),
                compiled,
            );
            assert!(
                result.is_err() || matches!(result, Ok(Value::Sealed { .. })),
                "{kind}, compiled={compiled}: must reject"
            );
            assert_eq!(
                snapshot(root),
                before,
                "{kind}, compiled={compiled}: rejection destroyed entries"
            );
        }
    }
}

#[test]
fn ordinary_replacement_and_same_inode_alias_follow_atomic_rename_semantics() {
    for compiled in [false, true] {
        for kind in ["file", "empty-directory", "hard-link", "symlink-entry"] {
            let fixture = tempfile::tempdir().unwrap();
            let root = fixture.path();
            if kind == "empty-directory" {
                std::fs::create_dir(root.join("source")).unwrap();
                std::fs::write(root.join("source/value"), b"source value").unwrap();
                std::fs::create_dir(root.join("destination")).unwrap();
            } else {
                std::fs::write(root.join("source"), b"source value").unwrap();
                if kind == "hard-link" {
                    std::fs::hard_link(root.join("source"), root.join("destination")).unwrap();
                } else if kind == "symlink-entry" {
                    std::fs::write(root.join("target"), b"retained target").unwrap();
                    std::os::unix::fs::symlink("target", root.join("destination")).unwrap();
                } else {
                    std::fs::write(root.join("destination"), b"old destination").unwrap();
                }
            }
            let before = snapshot(root);
            let result = perform(
                root,
                "io/fs::rename",
                "{:from \"source\" :to \"destination\" :overwrite true}",
                compiled,
            );
            assert!(is_nil(&result), "{kind}, compiled={compiled}: {result:?}");
            if kind == "hard-link" {
                assert_eq!(snapshot(root), before);
            } else {
                assert!(!root.join("source").exists());
                let value = if kind == "empty-directory" {
                    root.join("destination/value")
                } else {
                    root.join("destination")
                };
                assert_eq!(std::fs::read(value).unwrap(), b"source value");
                if kind == "symlink-entry" {
                    assert_eq!(
                        std::fs::read(root.join("target")).unwrap(),
                        b"retained target"
                    );
                    assert!(
                        !std::fs::symlink_metadata(root.join("destination"))
                            .unwrap()
                            .file_type()
                            .is_symlink()
                    );
                }
            }
        }
    }
}

#[test]
fn package_document_writer_uses_the_same_boundary_in_both_tiers() {
    for compiled in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let sandbox = fixture.path().join("sandbox");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&sandbox).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, sandbox.join("escape")).unwrap();
        for (path, allowed) in [
            ("escape/new/genesis.lock".to_string(), false),
            ("inside/genesis.lock".to_string(), true),
            (
                sandbox
                    .join("absolute/genesis.lock")
                    .to_str()
                    .unwrap()
                    .to_string(),
                true,
            ),
        ] {
            let operation = "core/pkg-low::init";
            let source = format!(
                "(core/effect::perform '{operation} {{:lock {path:?} :workspace \"boundary-control\"}} (fn (response) (core/effect::pure response)))"
            );
            let artifact = std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
                .or_else(|| std::env::var_os("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT"))
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../selfhost/toolchain.gc")
                });
            let policy = CapsPolicy::from_toml_str_with_selfhost_authority(
                &format!("allow = [{operation:?}]\n[op.{operation:?}]\nbase_dir = {:?}\ncreate_dirs = true\n", sandbox.to_str().unwrap()),
                gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
                Some(artifact.as_path()),
            ).unwrap();
            let forms = parse_module(&source).unwrap();
            let mut context = EvalCtx::with_step_limit(Some(2_000_000));
            let mut env = build_prelude(&mut context).env;
            let program = if compiled {
                eval_module_compiled(&mut context, &mut env, &forms)
            } else {
                eval_module(&mut context, &mut env, &forms)
            }
            .unwrap();
            let before = (snapshot(&sandbox), snapshot(&outside));
            let result = run(
                &mut context,
                &policy,
                program,
                hash_module(&forms),
                "package-boundary-control".to_string(),
            );
            if allowed {
                let output = result.expect("package init must reach the real document writer");
                assert!(
                    !matches!(output.value, Value::Sealed { .. }),
                    "package init failed: {:?}",
                    output.value
                );
                let bytes = std::fs::read(sandbox.join(path)).unwrap();
                let document: toml::Value =
                    toml::from_str(std::str::from_utf8(&bytes).unwrap()).unwrap();
                assert_eq!(document["workspace"].as_str(), Some("boundary-control"));
            } else {
                assert!(
                    result
                        .as_ref()
                        .map(|output| matches!(output.value, Value::Sealed { .. }))
                        .unwrap_or(true)
                );
                assert_eq!((snapshot(&sandbox), snapshot(&outside)), before);
            }
        }
    }
}

#[test]
fn rejected_root_entry_mutations_preserve_the_entire_sandbox() {
    for compiled in [false, true] {
        for (operation, payload) in [
            ("io/fs::write", "{:path \".\" :data \"forbidden\"}"),
            ("io/fs::remove", "{:path \".\" :recursive true}"),
            (
                "io/fs::rename",
                "{:from \".\" :to \"new/entry\" :overwrite true}",
            ),
            (
                "io/fs::rename",
                "{:from \"retained\" :to \".\" :overwrite true}",
            ),
        ] {
            let fixture = tempfile::tempdir().unwrap();
            std::fs::write(fixture.path().join("retained"), b"retained").unwrap();
            let before = snapshot(fixture.path());
            let result = perform(fixture.path(), operation, payload, compiled);
            assert!(result.is_err() || matches!(result, Ok(Value::Sealed { .. })));
            assert_eq!(snapshot(fixture.path()), before);
        }
    }
}

#[path = "support/replay.rs"]
mod replay_support;

#[test]
fn strict_replay_of_entry_operations_performs_no_filesystem_access() {
    for compiled in [false, true] {
        for (operation, payload) in [
            ("io/fs::read", "{:path \"source\"}"),
            ("io/fs::write", "{:path \"document\" :data \"document\"}"),
            ("io/fs::remove", "{:path \"source\"}"),
            (
                "io/fs::rename",
                "{:from \"source\" :to \"destination\" :overwrite true}",
            ),
        ] {
            let fixture = tempfile::tempdir().unwrap();
            let sandbox = fixture.path().join("sandbox");
            let outside = fixture.path().join("outside");
            std::fs::create_dir(&sandbox).unwrap();
            std::fs::create_dir(&outside).unwrap();
            std::fs::write(sandbox.join("source"), b"source").unwrap();
            let policy = CapsPolicy::from_toml_str(&format!(
                "allow = [{operation:?}]\n[op.{operation:?}]\nbase_dir = {:?}\n",
                sandbox.to_str().unwrap()
            ))
            .unwrap();
            let forms = parse_module(&format!("(core/effect::perform '{operation} {payload} (fn (response) (core/effect::pure response)))")).unwrap();
            let evaluate = || {
                let mut context = EvalCtx::with_step_limit(Some(2_000_000));
                let mut env = build_prelude(&mut context).env;
                let program = if compiled {
                    eval_module_compiled(&mut context, &mut env, &forms)
                } else {
                    eval_module(&mut context, &mut env, &forms)
                }
                .unwrap();
                (context, program)
            };
            let (mut context, program) = evaluate();
            let output = run(
                &mut context,
                &policy,
                program,
                hash_module(&forms),
                "entry-replay-control".to_string(),
            )
            .unwrap();
            assert!(!matches!(output.value, Value::Sealed { .. }));
            let log = gc_effects::EffectLog::from_term(&output.log.to_term()).unwrap();
            let expected = gc_kernel::value_hash(&output.value);
            std::fs::rename(&sandbox, fixture.path().join("retained-sandbox")).unwrap();
            std::os::unix::fs::symlink(&outside, &sandbox).unwrap();
            for name in ["source", "document", "destination"] {
                std::fs::write(outside.join(name), b"must not be observed or mutated").unwrap();
            }
            let before = snapshot(&outside);
            let (mut replay_context, replay_program) = evaluate();
            let replayed =
                replay_support::replay(&mut replay_context, replay_program, &log).unwrap();
            assert_eq!(gc_kernel::value_hash(&replayed), expected);
            assert_eq!(snapshot(&outside), before);
        }
    }
}

#[test]
fn symlink_parent_components_follow_physical_directory_semantics() {
    for compiled in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        std::fs::create_dir_all(root.join("branch/nested")).unwrap();
        std::fs::write(root.join("value"), b"wrong lexical target").unwrap();
        std::fs::write(root.join("branch/value"), b"physical target").unwrap();
        std::os::unix::fs::symlink("branch/nested", root.join("first")).unwrap();
        std::os::unix::fs::symlink("first/../value", root.join("link")).unwrap();
        let expected = std::fs::read(root.join("link")).unwrap();
        assert_eq!(expected, b"physical target");
        let result = perform(root, "io/fs::read", "{:path \"link\"}", compiled).unwrap();
        assert!(
            matches!(result.as_data(), Some(gc_coreform::Term::Bytes(bytes)) if bytes == &expected),
            "compiled={compiled}: physical resolution disagrees: {result:?}"
        );
    }
}

#[test]
fn symlink_directory_requirements_do_not_turn_files_into_directories() {
    for compiled in [false, true] {
        for target in ["file/../value", "file/.", "file/"] {
            let fixture = tempfile::tempdir().unwrap();
            std::fs::write(fixture.path().join("file"), b"file").unwrap();
            std::fs::write(fixture.path().join("value"), b"must not be selected").unwrap();
            std::os::unix::fs::symlink(target, fixture.path().join("link")).unwrap();
            assert!(std::fs::read(fixture.path().join("link")).is_err());
            let before = snapshot(fixture.path());
            let result = perform(fixture.path(), "io/fs::read", "{:path \"link\"}", compiled);
            assert!(
                result.is_err() || matches!(result, Ok(Value::Sealed { .. })),
                "target={target}, compiled={compiled}"
            );
            assert_eq!(snapshot(fixture.path()), before);
        }
    }
}

#[test]
fn missing_link_ancestor_cannot_hide_a_parent_escape_before_creation() {
    for compiled in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let sandbox = fixture.path().join("sandbox");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&sandbox).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink("missing/../../outside", sandbox.join("escape")).unwrap();
        let before = snapshot(fixture.path());
        let result = perform(
            &sandbox,
            "io/fs::mkdir",
            "{:path \"escape/new\" :parents true}",
            compiled,
        );
        assert!(result.is_err() || matches!(result, Ok(Value::Sealed { .. })));
        assert_eq!(snapshot(fixture.path()), before);
    }
}
