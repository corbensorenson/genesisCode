#![cfg(target_os = "wasi")]
//! Actual-WASI controls. The host supplies link fixtures and independently
//! checks identity/content preservation after the bounded test process exits.
use gc_coreform::{Term, hash_module, parse_module};
use gc_effects::{CapsPolicy, EffectsError, RunResult, run};
use gc_kernel::{EvalCtx, Value, eval_module, eval_module_compiled};
use gc_prelude::{SelfhostBootstrapMode, build_prelude};
use std::path::{Path, PathBuf};

#[path = "support/document_reader_controls.rs"]
mod document_reader_controls;
#[path = "support/replay.rs"]
mod replay_support;

fn fixture(name: &str, compiled: bool) -> PathBuf {
    PathBuf::from(std::env::var("GENESIS_WASI_FS_FIXTURES").unwrap())
        .join(format!("{name}-{}", u8::from(compiled)))
}

fn evaluate(operation: &str, payload: &str, compiled: bool) -> (EvalCtx, Value, [u8; 32]) {
    let forms = parse_module(&format!(
        "(core/effect::perform '{operation} {payload} (fn (response) (core/effect::pure response)))"
    ))
    .unwrap();
    let mut context = EvalCtx::with_step_limit(Some(2_000_000));
    let mut environment = build_prelude(&mut context).env;
    let program = if compiled {
        eval_module_compiled(&mut context, &mut environment, &forms)
    } else {
        eval_module(&mut context, &mut environment, &forms)
    }
    .unwrap();
    (context, program, hash_module(&forms))
}

fn perform(
    base: &Path,
    operation: &str,
    payload: &str,
    compiled: bool,
) -> Result<RunResult, EffectsError> {
    thread_local! {
        static POLICIES: std::cell::RefCell<std::collections::BTreeMap<PathBuf, CapsPolicy>> =
            const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    }
    let policy = POLICIES.with(|policies| {
        let mut policies = policies.borrow_mut();
        policies.entry(base.to_path_buf()).or_insert_with(|| {
            let operations = ["io/fs::read", "io/fs::write", "io/fs::stat", "io/fs::list",
                "io/fs::mkdir", "io/fs::remove", "io/fs::rename", "core/pkg-low::init",
                "core/pkg-low::load-lock"];
            let mut document = format!("allow = {operations:?}\n");
            for op in operations {
                document.push_str(&format!("[op.{op:?}]\nbase_dir = {:?}\ncreate_dirs = true\n", base.to_str().unwrap()));
            }
            document.push_str("[store]\ndir = \"/controls/runtime/store\"\n[refs]\npath = \"/controls/runtime/refs.gc\"\n");
            let artifact = PathBuf::from(std::env::var("GENESIS_TEST_SELFHOST_ARTIFACT").unwrap());
            CapsPolicy::from_toml_str_with_selfhost_authority(&document,
                SelfhostBootstrapMode::ArtifactOnly, Some(&artifact)).unwrap()
        }).clone()
    });
    let (mut context, program, identity) = evaluate(operation, payload, compiled);
    run(
        &mut context,
        &policy,
        program,
        identity,
        "wasi-rooted-control".into(),
    )
}

fn succeeds(result: Result<RunResult, EffectsError>) -> Value {
    let result = result.unwrap();
    assert!(
        !matches!(result.value, Value::Sealed { .. }),
        "unexpected sealed error: {:?}",
        result.value
    );
    result.value
}

fn rejects(result: Result<RunResult, EffectsError>) {
    assert!(
        result
            .map(|output| matches!(output.value, Value::Sealed { .. }))
            .unwrap_or(true)
    );
}

#[test]
fn wasi_actual_ordinary_descriptor_operations() {
    for compiled in [false, true] {
        let root = fixture("ordinary", compiled);
        for (operation, payload) in [
            ("io/fs::mkdir", "{:path \"inside/new\" :parents true}"),
            (
                "io/fs::write",
                "{:path \"inside/new/value\" :data \"value\"}",
            ),
            (
                "io/fs::rename",
                "{:from \"inside/new/value\" :to \"replacement\" :overwrite true}",
            ),
        ] {
            succeeds(perform(&root, operation, payload, compiled));
        }
        let read = succeeds(perform(
            &root,
            "io/fs::read",
            "{:path \"replacement\"}",
            compiled,
        ));
        assert!(matches!(read.as_data(),Some(Term::Bytes(bytes)) if bytes.as_ref()==b"value"));
        let stat = succeeds(perform(
            &root,
            "io/fs::stat",
            "{:path \"replacement\"}",
            compiled,
        ));
        assert!(
            matches!(stat.as_data(),Some(Term::Map(fields)) if fields.get(&gc_coreform::TermOrdKey(Term::symbol(":kind")))==Some(&Term::symbol("file")))
        );
        let Term::Map(fields) = stat.as_data().unwrap() else {
            panic!("stat envelope")
        };
        assert_eq!(
            fields.get(&gc_coreform::TermOrdKey(Term::symbol(":readonly"))),
            Some(&Term::Bool(
                std::fs::metadata(root.join("replacement"))
                    .unwrap()
                    .permissions()
                    .readonly()
            ))
        );
        let list = succeeds(perform(&root, "io/fs::list", "{:path \".\"}", compiled));
        assert!(matches!(list.as_data(),Some(Term::Vector(rows)) if rows.len()==2));
        succeeds(perform(
            &root,
            "io/fs::remove",
            "{:path \"inside\" :recursive true}",
            compiled,
        ));
        succeeds(perform(
            &root,
            "io/fs::remove",
            "{:path \"replacement\"}",
            compiled,
        ));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }
}

#[test]
fn wasi_actual_rejections_preserve_host_fixture_identity() {
    for compiled in [false, true] {
        let root = fixture("denied", compiled);
        for (operation, payload) in [
            (
                "io/fs::write",
                "{:path \"escape/new/entry\" :data \"forbidden\"}",
            ),
            ("io/fs::mkdir", "{:path \"escape/new/entry\" :parents true}"),
            ("io/fs::read", "{:path \"escape/retained\"}"),
            ("io/fs::stat", "{:path \"escape/retained\"}"),
            ("io/fs::list", "{:path \"escape\"}"),
            (
                "io/fs::remove",
                "{:path \"escape/retained\" :recursive true}",
            ),
            (
                "io/fs::rename",
                "{:from \"source\" :to \"escape/new/entry\" :overwrite true}",
            ),
            (
                "io/fs::rename",
                "{:from \"source\" :to \"new/entry\" :overwrite false}",
            ),
            (
                "io/fs::rename",
                "{:from \"absent\" :to \"new/entry\" :overwrite true}",
            ),
            (
                "io/fs::rename",
                "{:from \"source\" :to \"directory\" :overwrite true}",
            ),
            ("io/fs::write", "{:path \".\" :data \"forbidden\"}"),
            ("io/fs::remove", "{:path \".\" :recursive true}"),
        ] {
            rejects(perform(&root, operation, payload, compiled));
        }
        rejects(perform(
            &root,
            "io/fs::remove",
            "{:path \"deep\" :recursive true}",
            compiled,
        ));
        // Ancestor and final read links remain outside the WASI read profile.
        rejects(perform(
            &root,
            "io/fs::read",
            "{:path \"inside-link\"}",
            compiled,
        ));
    }
}

#[test]
fn wasi_actual_final_links_are_entries() {
    for compiled in [false, true] {
        let root = fixture("entries", compiled);
        for link in [
            "file-link",
            "directory-link",
            "dangling-link",
            "outside-link",
        ] {
            let stat = succeeds(perform(
                &root,
                "io/fs::stat",
                &format!("{{:path {link:?}}}"),
                compiled,
            ));
            assert!(
                matches!(stat.as_data(),Some(Term::Map(fields)) if fields.get(&gc_coreform::TermOrdKey(Term::symbol(":kind")))==Some(&Term::symbol("symlink")))
            );
            succeeds(perform(
                &root,
                "io/fs::remove",
                &format!("{{:path {link:?} :recursive true}}"),
                compiled,
            ));
        }
        succeeds(perform(
            &root,
            "io/fs::rename",
            "{:from \"source\" :to \"source\" :overwrite true}",
            compiled,
        ));
        assert_eq!(std::fs::read(root.join("source")).unwrap(), b"source");
        succeeds(perform(
            &root,
            "io/fs::rename",
            "{:from \"source\" :to \"destination-link\" :overwrite true}",
            compiled,
        ));
        assert_eq!(
            std::fs::read(root.join("destination-link")).unwrap(),
            b"source"
        );
        assert_eq!(std::fs::read(root.join("target")).unwrap(), b"retained");
        assert_eq!(
            std::fs::read(root.join("directory/retained")).unwrap(),
            b"retained"
        );
    }
}

#[test]
fn wasi_actual_package_writer_replaces_entries_and_cleans_failure() {
    for compiled in [false, true] {
        let root = fixture("documents", compiled);
        let artifact = PathBuf::from(std::env::var("GENESIS_TEST_SELFHOST_ARTIFACT").unwrap());
        document_reader_controls::controls(&root, &artifact, compiled);
        for destination in [
            "genesis.lock".to_string(),
            root.join("absolute/genesis.lock")
                .to_str()
                .unwrap()
                .to_string(),
        ] {
            succeeds(perform(
                &root,
                "core/pkg-low::init",
                &format!("{{:lock {destination:?} :workspace \"wasi-control\"}}"),
                compiled,
            ));
            let bytes = std::fs::read(root.join(&destination)).unwrap();
            let document: toml::Value =
                toml::from_str(std::str::from_utf8(&bytes).unwrap()).unwrap();
            assert_eq!(document["workspace"].as_str(), Some("wasi-control"));
            let loaded = succeeds(perform(
                &root,
                "core/pkg-low::load-lock",
                &format!("{{:lock {destination:?}}}"),
                compiled,
            ));
            assert!(matches!(loaded.as_data(), Some(Term::Map(_))));
        }
        for source in [
            "escape/genesis.lock",
            "../outside/genesis.lock",
            "genesis.lock/",
            "/controls/outside/genesis.lock",
        ] {
            rejects(perform(
                &root,
                "core/pkg-low::load-lock",
                &format!("{{:lock {source:?}}}"),
                compiled,
            ));
        }
        rejects(perform(
            &root,
            "core/pkg-low::init",
            "{:lock \"escape/new/genesis.lock\" :workspace \"forbidden\"}",
            compiled,
        ));
        rejects(perform(
            &root,
            "core/pkg-low::init",
            "{:lock \"directory\" :workspace \"forbidden\"}",
            compiled,
        ));
        for entry in std::fs::read_dir(&root).unwrap() {
            assert!(
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".genesis-write.")
            );
        }
    }
}

#[test]
fn wasi_actual_strict_replay_ignores_current_filesystem() {
    for compiled in [false, true] {
        let root = fixture("replay", compiled);
        for (operation, payload) in [
            ("io/fs::read", "{:path \"source\"}"),
            ("io/fs::write", "{:path \"document\" :data \"document\"}"),
            (
                "io/fs::rename",
                "{:from \"source\" :to \"destination\" :overwrite true}",
            ),
            ("io/fs::remove", "{:path \"destination\"}"),
        ] {
            let output = perform(&root, operation, payload, compiled).unwrap();
            assert!(!matches!(output.value, Value::Sealed { .. }));
            let log = gc_effects::EffectLog::from_term(&output.log.to_term()).unwrap();
            let expected = gc_kernel::value_hash(&output.value);
            std::fs::rename(&root, root.with_extension("held")).unwrap();
            let (mut context, program, _) = evaluate(operation, payload, compiled);
            let replayed = replay_support::replay(&mut context, program, &log).unwrap();
            assert_eq!(gc_kernel::value_hash(&replayed), expected);
            assert!(!root.exists());
            std::fs::rename(root.with_extension("held"), &root).unwrap();
        }
    }
}
