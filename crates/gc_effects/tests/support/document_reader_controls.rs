use gc_coreform::{Term, TermOrdKey, hash_module, parse_module, print_term};
use gc_effects::{ArtifactStore, CapsPolicy, run};
use gc_kernel::{EvalCtx, Value, eval_module, eval_module_compiled};
use gc_prelude::{SelfhostBootstrapMode, build_prelude};
use std::path::Path;

fn perform(policy: &CapsPolicy, operation: &str, payload: &str, compiled: bool) -> Value {
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
    run(
        &mut context,
        policy,
        program,
        hash_module(&forms),
        "document-reader-control".into(),
    )
    .unwrap()
    .value
}

fn rejects(value: Value, code: &str) {
    assert!(matches!(value, Value::Sealed { .. }), "{value:?}");
    assert!(format!("{value:?}").contains(code), "{value:?}");
}

pub(crate) fn controls(root: &Path, artifact: &Path, compiled: bool) {
    let store_path = root.join("reader-store");
    let store = ArtifactStore::open(&store_path).unwrap();
    let base = store
        .put_bytes(print_term(&Term::Int(42.into())).as_bytes())
        .unwrap();
    let patch = "{:type :vcs/patch :v 1 :ops []}";
    std::fs::write(root.join("patch.gc"), patch).unwrap();
    std::fs::write(root.join("response.gc"), "{:ok true}").unwrap();
    let policy = CapsPolicy::from_toml_str_with_selfhost_authority(&format!(
        "allow = [\"core/vcs-low::apply\", \"gpu/compute::limits\"]\n[op.\"core/vcs-low::apply\"]\nbase_dir = {:?}\nmax_bytes = 256\n[op.\"gpu/compute::limits\"]\nbase_dir = {:?}\nwasi_bridge_profile = true\nwasi_bridge_response_file = \"response.gc\"\nmax_bytes = 32\n[store]\ndir = {:?}\n[refs]\npath = {:?}\n",
        root.to_str().unwrap(), root.to_str().unwrap(), store_path.to_str().unwrap(), root.join("reader-refs.gc").to_str().unwrap()
    ), SelfhostBootstrapMode::ArtifactOnly, Some(artifact)).unwrap();
    for name in [
        "patch.gc".to_owned(),
        root.join("patch.gc").to_str().unwrap().to_owned(),
    ] {
        let result = perform(
            &policy,
            "core/vcs-low::apply",
            &format!("{{:base {base:?} :patch {name:?} :store false}}"),
            compiled,
        );
        assert!(
            matches!(result.as_data(), Some(Term::Map(fields)) if fields.get(&TermOrdKey(Term::symbol(":snapshot"))) == Some(&Term::Str(base.clone()))),
            "{result:?}"
        );
    }
    for name in ["escape/patch.gc", "../outside/patch.gc", "patch.gc/"] {
        rejects(
            perform(
                &policy,
                "core/vcs-low::apply",
                &format!("{{:base {base:?} :patch {name:?} :store false}}"),
                compiled,
            ),
            "core/caps/path-escape",
        );
    }
    std::fs::write(root.join("large-patch.gc"), " ".repeat(1024 * 1024)).unwrap();
    rejects(
        perform(
            &policy,
            "core/vcs-low::apply",
            &format!("{{:base {base:?} :patch \"large-patch.gc\" :store false}}"),
            compiled,
        ),
        "core/vcs/patch-too-large",
    );
    std::fs::write(root.join("invalid-patch.gc"), [0xff]).unwrap();
    rejects(
        perform(
            &policy,
            "core/vcs-low::apply",
            &format!("{{:base {base:?} :patch \"invalid-patch.gc\" :store false}}"),
            compiled,
        ),
        "core/vcs/io-error",
    );
    for wire in [
        "{:ok true}".to_owned(),
        "10\n{:ok true}".to_owned(),
        "{gpu/compute::limits {:ok true}}".to_owned(),
    ] {
        std::fs::write(root.join("response.gc"), wire).unwrap();
        let response = perform(&policy, "gpu/compute::limits", "{}", compiled);
        assert!(
            matches!(response.as_data(), Some(Term::Map(fields)) if fields.get(&TermOrdKey(Term::symbol(":ok"))) == Some(&Term::Bool(true))),
            "{response:?}"
        );
    }
    std::fs::write(
        root.join("response.gc"),
        " ".repeat(1024 * 1024) + "{:ok true}",
    )
    .unwrap();
    rejects(
        perform(&policy, "gpu/compute::limits", "{}", compiled),
        "gpu/bridge-response-too-large",
    );
    std::fs::write(root.join("response.gc"), [0xff]).unwrap();
    rejects(
        perform(&policy, "gpu/compute::limits", "{}", compiled),
        "wasi/bridge-stdout-utf8",
    );
}
