#![cfg(unix)]
//! Local public-runner controls; these do not supply independent acceptance.
use gc_coreform::{Term, TermOrdKey, hash_module, parse_module};
use gc_effects::{CapsPolicy, run};
use gc_kernel::{EvalCtx, Value, eval_module, eval_module_compiled};
use gc_prelude::{SelfhostBootstrapMode, build_prelude};
use std::path::Path;

fn perform(policy: &CapsPolicy, operation: &str, path: &str, compiled: bool) -> Value {
    let forms = parse_module(&format!(
        "(core/effect::perform '{operation} {{:lock {path:?}}} (fn (response) (core/effect::pure response)))"
    )).unwrap();
    let mut context = EvalCtx::with_step_limit(Some(2_000_000));
    let mut environment = build_prelude(&mut context).env;
    let program = if compiled {
        eval_module_compiled(&mut context, &mut environment, &forms)
    } else {
        eval_module(&mut context, &mut environment, &forms)
    }
    .unwrap();
    run(
        &mut context,
        policy,
        program,
        hash_module(&forms),
        "pkg-lock-read-control".into(),
    )
    .unwrap()
    .value
}

#[test]
fn package_lock_reads_use_artifact_authority_and_confined_files() {
    for compiled in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let document = b"version = 1\nworkspace = \"inside\"\n";
        std::fs::write(root.join("genesis.lock"), document).unwrap();
        std::fs::write(
            fixture.path().join("outside.lock"),
            b"version = 1\nworkspace = \"outside\"\n",
        )
        .unwrap();
        std::os::unix::fs::symlink("genesis.lock", root.join("inside-link")).unwrap();
        std::os::unix::fs::symlink(
            fixture.path().join("outside.lock"),
            root.join("outside-link"),
        )
        .unwrap();
        let outside_before = std::fs::read(fixture.path().join("outside.lock")).unwrap();
        let artifact = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/toolchain.gc");
        let policy = CapsPolicy::from_toml_str_with_selfhost_authority(
            &format!("allow = [\"core/pkg-low::load-lock\", \"core/pkg-low::list\"]\n[op.\"core/pkg-low::load-lock\"]\nbase_dir = {:?}\n[op.\"core/pkg-low::list\"]\nbase_dir = {:?}\n[store]\ndir = {:?}\n[refs]\npath = {:?}\n",
                root.to_str().unwrap(), root.to_str().unwrap(), fixture.path().join("store").to_str().unwrap(), fixture.path().join("refs.gc").to_str().unwrap()),
            SelfhostBootstrapMode::ArtifactOnly, Some(&artifact)
        ).unwrap();
        for source in [
            "genesis.lock".to_owned(),
            "inside-link".to_owned(),
            root.join("genesis.lock").to_str().unwrap().to_owned(),
        ] {
            let loaded = perform(&policy, "core/pkg-low::load-lock", &source, compiled);
            assert!(
                matches!(loaded.as_data(), Some(Term::Map(fields)) if fields.get(&TermOrdKey(Term::symbol(":workspace"))) == Some(&Term::Str("inside".into()))),
                "{loaded:?}"
            );
            assert!(!matches!(
                perform(&policy, "core/pkg-low::list", &source, compiled),
                Value::Sealed { .. }
            ));
        }
        for source in ["outside-link", "../outside.lock", "genesis.lock/"] {
            assert!(matches!(
                perform(&policy, "core/pkg-low::load-lock", source, compiled),
                Value::Sealed { .. }
            ));
        }
        std::fs::File::create(root.join("large.lock"))
            .unwrap()
            .set_len(4 * 1024 * 1024 + 1)
            .unwrap();
        assert!(matches!(
            perform(&policy, "core/pkg-low::load-lock", "large.lock", compiled),
            Value::Sealed { .. }
        ));
        assert_eq!(
            std::fs::read(fixture.path().join("outside.lock")).unwrap(),
            outside_before
        );
    }
}
